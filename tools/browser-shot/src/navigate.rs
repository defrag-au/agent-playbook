//! Navigation with a deadline that is the caller's, and a failure that says what happened.
//!
//! `headless_chrome`'s `wait_until_navigated` waits on the tab's own default timeout — 20 s
//! unless something calls `Tab::set_default_timeout` — and fails with a bare "The event waited
//! for never came". This waits for the same event (the main frame's `networkAlmostIdle`) against
//! the caller's timeout, gives up early when the renderer crashes, and describes a timeout well
//! enough to tell a hung main thread from a page that is still loading.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use headless_chrome::browser::tab::Tab;

use crate::collect::Collector;
use crate::report::{epoch_ms, Predicate};

const POLL: Duration = Duration::from_millis(50);
/// How long a hung main thread gets to answer `Runtime.evaluate("1")` before it is called hung.
const RESPONSIVE_MS: u64 = 2000;
const IN_FLIGHT_SHOWN: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The renderer went away: `Inspector.targetCrashed`, or the inspector detached.
    Crashed(String),
    /// The navigation did not complete within the timeout; `detail` says how far it got.
    TimedOut { after_ms: u128, detail: String },
    /// `Page.navigate` itself failed, e.g. `net::ERR_CONNECTION_REFUSED`.
    Failed(String),
}

impl Failure {
    pub fn reason(&self) -> String {
        match self {
            Self::Crashed(detail) => format!("navigation crashed: {detail}"),
            Self::TimedOut { detail, .. } => format!("navigation timed out: {detail}"),
            Self::Failed(detail) => format!("navigation failed: {detail}"),
        }
    }

    /// The short form, for the `navigation` predicate.
    pub fn summary(&self) -> String {
        match self {
            Self::Crashed(detail) => format!("crashed: {detail}"),
            Self::TimedOut { after_ms, .. } => format!("timed out after {after_ms} ms"),
            Self::Failed(detail) => format!("failed: {detail}"),
        }
    }

    pub fn is_crash(&self) -> bool {
        matches!(self, Self::Crashed(_))
    }
}

/// Run `work` on its own thread and wait at most `deadline` for it.
///
/// A CDP call to a renderer whose main thread never yields does not return until the
/// transport's idle timeout (minutes), so anything sent to a page that may be hung goes through
/// here. A call that misses the deadline is abandoned; it ends when the browser is dropped.
pub fn with_deadline<T, F>(deadline: Duration, work: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    receiver.recv_timeout(deadline).ok()
}

/// Navigate `tab` to `url` and wait for the main frame's `networkAlmostIdle`, for at most
/// `timeout` in total. Returns the navigation predicate either way.
pub fn navigate(
    tab: &Arc<Tab>,
    collector: &Arc<Mutex<Collector>>,
    url: &str,
    timeout: Duration,
) -> (Predicate, Option<Failure>) {
    let started = Instant::now();
    let result = navigate_inner(tab, collector, url, timeout, started);
    let predicate = Predicate {
        name: "navigation".to_owned(),
        value: match &result {
            Ok(()) => "networkAlmostIdle".to_owned(),
            Err(failure) => failure.summary(),
        },
        passed: result.is_ok(),
        waited_ms: started.elapsed().as_millis() as u64,
        t_ms: epoch_ms(),
    };
    (predicate, result.err())
}

fn navigate_inner(
    tab: &Arc<Tab>,
    collector: &Arc<Mutex<Collector>>,
    url: &str,
    timeout: Duration,
    started: Instant,
) -> Result<(), Failure> {
    // Keep the library's own wait meaningful for any caller that still uses it.
    tab.set_default_timeout(timeout);
    if let Ok(mut collector) = collector.lock() {
        collector.begin_navigation(tab.get_target_id());
    }

    let navigating = Arc::clone(tab);
    let target = url.to_owned();
    match with_deadline(timeout, move || {
        navigating
            .navigate_to(&target)
            .map(|_| ())
            .map_err(|error| format!("{error:#}"))
    }) {
        None => {
            return Err(crash(collector).unwrap_or_else(|| Failure::TimedOut {
                after_ms: timeout.as_millis(),
                detail: format!(
                    "Page.navigate did not return within {} ms",
                    timeout.as_millis()
                ),
            }))
        }
        Some(Err(error)) => return Err(crash(collector).unwrap_or(Failure::Failed(error))),
        Some(Ok(())) => {}
    }

    loop {
        if let Some(failure) = crash(collector) {
            return Err(failure);
        }
        let navigating = collector.lock().map_or(true, |state| state.navigating());
        if !navigating {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            break;
        }
        std::thread::sleep(POLL);
    }

    let after_ms = started.elapsed().as_millis();
    // The renderer may have crashed while the timeout was being reached; a crash wins.
    let responsive = main_thread_responsive(tab, Duration::from_millis(RESPONSIVE_MS));
    if let Some(failure) = crash(collector) {
        return Err(failure);
    }
    Err(Failure::TimedOut {
        after_ms,
        detail: describe_timeout(collector, timeout, responsive),
    })
}

fn crash(collector: &Arc<Mutex<Collector>>) -> Option<Failure> {
    collector.lock().ok().and_then(|state| {
        state
            .crash()
            .map(|crash| Failure::Crashed(crash.to_owned()))
    })
}

/// `Some(true)` if the page answers a trivial evaluation within `deadline`, `Some(false)` if it
/// does not (a main thread that never yields), `None` if the call failed outright.
pub fn main_thread_responsive(tab: &Arc<Tab>, deadline: Duration) -> Option<bool> {
    let tab = Arc::clone(tab);
    match with_deadline(deadline, move || tab.evaluate("1", false).is_ok()) {
        None => Some(false),
        Some(true) => Some(true),
        Some(false) => None,
    }
}

fn describe_timeout(
    collector: &Arc<Mutex<Collector>>,
    timeout: Duration,
    responsive: Option<bool>,
) -> String {
    let (lifecycle, in_flight) = collector
        .lock()
        .map(|state| (state.lifecycle().to_vec(), state.in_flight_urls()))
        .unwrap_or_default();
    timeout_text(timeout, &lifecycle, &in_flight, responsive)
}

fn timeout_text(
    timeout: Duration,
    lifecycle: &[String],
    in_flight: &[String],
    responsive: Option<bool>,
) -> String {
    let mut parts = vec![format!(
        "no networkAlmostIdle within {} ms",
        timeout.as_millis()
    )];
    parts.push(match responsive {
        Some(false) => {
            format!("main thread hung (Runtime.evaluate did not answer within {RESPONSIVE_MS} ms)")
        }
        Some(true) => "main thread responsive".to_owned(),
        None => "main thread state unknown (Runtime.evaluate failed)".to_owned(),
    });
    parts.push(if lifecycle.is_empty() {
        "main frame lifecycle: none".to_owned()
    } else {
        format!("main frame lifecycle: {}", lifecycle.join(" "))
    });
    if !in_flight.is_empty() {
        let shown = in_flight
            .iter()
            .take(IN_FLIGHT_SHOWN)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let more = in_flight.len().saturating_sub(IN_FLIGHT_SHOWN);
        parts.push(format!(
            "{} request(s) in flight: {shown}{}",
            in_flight.len(),
            if more > 0 {
                format!(" (+{more} more)")
            } else {
                String::new()
            }
        ));
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hung_page_reads_as_a_hang() {
        let text = timeout_text(
            Duration::from_millis(30_000),
            &["init@5ms".to_owned(), "DOMContentLoaded@40ms".to_owned()],
            &[],
            Some(false),
        );
        assert!(text.starts_with("no networkAlmostIdle within 30000 ms"));
        assert!(text.contains("main thread hung"));
        assert!(text.contains("lifecycle: init@5ms DOMContentLoaded@40ms"));
        assert!(!text.contains("in flight"));
    }

    #[test]
    fn a_loading_page_names_what_is_in_flight() {
        let urls = (0..5).map(|n| format!("http://x/{n}")).collect::<Vec<_>>();
        let text = timeout_text(Duration::from_millis(1000), &[], &urls, Some(true));
        assert!(text.contains("main thread responsive"));
        assert!(
            text.contains("5 request(s) in flight: http://x/0, http://x/1, http://x/2 (+2 more)")
        );
    }

    #[test]
    fn reasons_name_their_kind() {
        assert!(Failure::Crashed("x".into())
            .reason()
            .starts_with("navigation crashed"));
        let timed_out = Failure::TimedOut {
            after_ms: 30_000,
            detail: "x".into(),
        };
        assert_eq!(timed_out.reason(), "navigation timed out: x");
        assert_eq!(timed_out.summary(), "timed out after 30000 ms");
        assert!(Failure::Failed("x".into())
            .reason()
            .starts_with("navigation failed"));
        assert!(Failure::Crashed("x".into()).is_crash());
    }

    #[test]
    fn a_missed_deadline_is_none() {
        assert_eq!(with_deadline(Duration::from_millis(20), || 7), Some(7));
        assert_eq!(
            with_deadline(Duration::from_millis(20), || {
                std::thread::sleep(Duration::from_millis(200));
                7
            }),
            None
        );
    }
}
