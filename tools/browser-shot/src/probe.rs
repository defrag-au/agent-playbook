//! Readiness predicates. Every wait is a condition with a timeout, and a timeout names the
//! predicate and the value it last saw.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use headless_chrome::browser::tab::Tab;
use serde_json::Value;

use crate::collect::{remote_text, Collector};
use crate::report::Predicate;

const POLL: Duration = Duration::from_millis(50);

pub fn evaluate_labeled(tab: &Tab, expression: &str) -> Result<(Value, String)> {
    let object = tab.evaluate(expression, false)?;
    Ok((
        object.value.clone().unwrap_or(Value::Null),
        remote_text(&object),
    ))
}

pub fn evaluate_text(tab: &Tab, expression: &str) -> String {
    match tab.evaluate(expression, false) {
        Ok(object) => remote_text(&object),
        Err(error) => format!("error: {error}"),
    }
}

pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

pub fn wait_for_fonts(tab: &Tab, timeout: Duration) -> Predicate {
    let started = Instant::now();
    let mut status;

    loop {
        status = evaluate_text(tab, "document.fonts ? document.fonts.status : 'loaded'");
        if status == "loaded" {
            // One frame after the swap, not just the promise resolving.
            let _ = tab.evaluate(
                "new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))",
                true,
            );
            return Predicate::passed("fonts.ready", status, started);
        }
        if started.elapsed() >= timeout {
            return Predicate::failed("fonts.ready", status, started);
        }
        std::thread::sleep(POLL);
    }
}

pub fn wait_for_predicate(tab: &Tab, expression: &str, timeout: Duration) -> Predicate {
    let started = Instant::now();
    let mut last;

    loop {
        match evaluate_labeled(tab, expression) {
            Ok((value, label)) => {
                last = label;
                if truthy(&value) {
                    return Predicate::passed(expression, last, started);
                }
            }
            Err(error) => last = format!("error: {error}"),
        }
        if started.elapsed() >= timeout {
            return Predicate::failed(expression, last, started);
        }
        std::thread::sleep(POLL);
    }
}

pub fn wait_for_frames(tab: &Tab, counter: &str, frames: u32, timeout: Duration) -> Predicate {
    let started = Instant::now();
    let mut last;
    let mut origin: Option<f64> = None;

    loop {
        match evaluate_labeled(tab, counter) {
            Ok((value, label)) => {
                last = label;
                if let Some(current) = value.as_f64() {
                    let origin = *origin.get_or_insert(current);
                    if frames == 0 || current >= origin + f64::from(frames) {
                        return Predicate::passed(
                            counter,
                            format!("{current} (waited {frames})"),
                            started,
                        );
                    }
                    last = format!("{current} (needs {})", origin + f64::from(frames));
                }
            }
            Err(error) => last = format!("error: {error}"),
        }
        if started.elapsed() >= timeout {
            return Predicate::failed(counter, last, started);
        }
        std::thread::sleep(POLL);
    }
}

pub fn wait_for_network_idle(
    collector: &Arc<Mutex<Collector>>,
    window: Duration,
    timeout: Duration,
) -> Predicate {
    let started = Instant::now();
    let mut last;

    loop {
        let Ok(collector) = collector.lock() else {
            return Predicate::failed("network.idle", "collector poisoned", started);
        };

        if collector.idle_for(window) {
            return Predicate::passed(
                "network.idle",
                format!("0 in flight for {} ms", window.as_millis()),
                started,
            );
        }
        last = format!("{} in flight", collector.in_flight());
        drop(collector);

        if started.elapsed() >= timeout {
            return Predicate::failed("network.idle", last, started);
        }
        std::thread::sleep(POLL);
    }
}
