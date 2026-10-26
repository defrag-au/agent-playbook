//! One child process in a PTY this tool owns.
//!
//! Nothing here attaches to an existing terminal, keeps a session, or shells out: the program is
//! `exec`'d directly with argv, the only writes are bytes to its stdin, and the only screen read is
//! the emulator's own. The child is killed by the handle that spawned it.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use vt100::Parser;

use crate::bin::Resolved;
use crate::report::{ExitInfo, ProgramInfo};
use crate::scenario::{Size, TuiSpec, Wait};
use crate::screen::{Snapshot, Warning};

pub struct Tui {
    pub name: String,
    pub info: ProgramInfo,
    pub pid: Option<u32>,
    child: Box<dyn Child + Send + Sync>,
    writer: Option<Box<dyn Write + Send>>,
    master: Box<dyn MasterPty + Send>,
    parser: Parser,
    rx: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
    bytes: u64,
    chunks: u64,
    spawned_at: Instant,
    last_output: Instant,
    exit: Option<ExitInfo>,
    /// Set when `try_wait` reports the status, until the reader has delivered its last bytes.
    pending_exit: Option<ExitInfo>,
    exit_seen_at: Option<Instant>,
}

pub struct CaptureOutcome {
    pub snapshot: Snapshot,
    pub quiet_ms: u64,
    pub still_changing: bool,
}

pub enum WaitOutcome {
    Met { quiet_ms: u64, still_changing: bool },
    TimedOut { reason: String },
    Exited { reason: String },
}

impl Tui {
    pub fn spawn(spec: &TuiSpec, resolved: &Resolved, cwd: &std::path::Path) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: spec.size.rows,
                cols: spec.size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("open a pty")?;

        let mut command = CommandBuilder::new(&resolved.path);
        command.args(&spec.args);
        command.cwd(cwd);
        command.env("TERM", spec.term.as_str());
        // A TUI that checks for a known terminal should not be told it is inside a real one.
        command.env_remove("TERM_PROGRAM");

        let child = pair
            .slave
            .spawn_command(command)
            .with_context(|| format!("spawn {}", resolved.path.display()))?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("clone the pty reader")?;
        let writer = pair.master.take_writer().context("take the pty writer")?;
        let pid = child.process_id();

        let (tx, rx) = mpsc::channel();
        let reader = std::thread::Builder::new()
            .name(format!("tui-shot-read-{}", spec.name))
            .spawn(move || {
                let mut buffer = [0u8; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => {
                            if tx.send(buffer[..read].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            })
            .context("start the pty reader")?;

        if let Some(pid) = pid {
            crate::signals::register(pid);
        }

        let now = Instant::now();
        let mut tui = Tui {
            name: spec.name.clone(),
            info: resolved.info.clone(),
            pid,
            child,
            writer: Some(writer),
            master: pair.master,
            parser: Parser::new(spec.size.rows, spec.size.cols, 1000),
            rx,
            reader: Some(reader),
            bytes: 0,
            chunks: 0,
            spawned_at: now,
            last_output: now,
            exit: None,
            pending_exit: None,
            exit_seen_at: None,
        };
        tui.poll();
        Ok(tui)
    }

    pub fn exit(&self) -> Option<&ExitInfo> {
        self.exit.as_ref()
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.spawned_at.elapsed().as_millis() as u64
    }

    pub fn chunks(&self) -> u64 {
        self.chunks
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// True when output arrived, so a caller can avoid re-reading the screen when nothing changed.
    fn poll(&mut self) -> bool {
        let mut arrived = false;
        let mut disconnected = false;
        loop {
            match self.rx.try_recv() {
                Ok(chunk) => {
                    self.bytes += chunk.len() as u64;
                    self.chunks += 1;
                    self.parser.process(&chunk);
                    arrived = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if arrived {
            self.last_output = Instant::now();
        }

        if self.exit.is_none() {
            if self.pending_exit.is_none() {
                match self.child.try_wait() {
                    Ok(Some(status)) => {
                        self.pending_exit = Some(exit_info(status));
                        self.exit_seen_at = Some(Instant::now());
                    }
                    Ok(None) => {}
                    Err(error) => {
                        // A wait failure is not a reason to lose the capture; the process is gone
                        // either way, and the reader thread will have seen EOF.
                        let _ = error;
                    }
                }
            }
            if self.pending_exit.is_some() {
                // The reader sends its final chunk before it drops the channel. Wait for that
                // rather than photographing a frame missing its last bytes; if a grandchild keeps
                // the pty open, commit after a short grace anyway.
                let grace_elapsed = self
                    .exit_seen_at
                    .is_some_and(|seen| seen.elapsed() >= Duration::from_millis(50));
                if disconnected || grace_elapsed {
                    self.exit = self.pending_exit.take();
                    if let Some(pid) = self.pid {
                        crate::signals::unregister(pid);
                    }
                }
            }
        }
        arrived
    }

    fn quiet_for(&self) -> Duration {
        self.last_output.elapsed()
    }

    /// Wait until every condition holds and the screen has been still for `settle`, or until the
    /// bound expires. A condition that has held for `settle` counts even while output continues, so
    /// a screen with a clock or spinner does not hang the wait; the capture reports that it was
    /// still changing.
    pub fn wait(
        &mut self,
        waits: &[Wait],
        timeout: Duration,
        settle: Duration,
    ) -> Result<WaitOutcome> {
        let started = Instant::now();
        let mut held_since: Option<Instant> = None;

        loop {
            self.poll();
            let screen = self.parser.screen();

            let unsatisfied = waits
                .iter()
                .filter(|wait| {
                    !condition_holds(
                        wait,
                        screen,
                        self.quiet_for(),
                        self.chunks > 0,
                        self.exit.is_some(),
                    )
                })
                .map(ToString::to_string)
                .collect::<Vec<_>>();

            if unsatisfied.is_empty() {
                if waits.iter().any(|wait| matches!(wait, Wait::Exit)) {
                    return Ok(WaitOutcome::Met {
                        quiet_ms: self.quiet_for().as_millis() as u64,
                        still_changing: false,
                    });
                }
                let since = *held_since.get_or_insert_with(Instant::now);
                let quiet = self.quiet_for();
                if quiet >= settle || since.elapsed() >= settle {
                    return Ok(WaitOutcome::Met {
                        quiet_ms: quiet.as_millis() as u64,
                        still_changing: quiet < settle,
                    });
                }
            } else {
                held_since = None;
            }

            if self.exit.is_some() && !waits.iter().any(|wait| matches!(wait, Wait::Exit)) {
                return Ok(WaitOutcome::Exited {
                    reason: format!(
                        "{} (child {})",
                        unsatisfied.join(" · "),
                        self.exit
                            .as_ref()
                            .map_or_else(|| "exited".to_owned(), ExitInfo::summary)
                    ),
                });
            }

            if started.elapsed() >= timeout {
                return Ok(WaitOutcome::TimedOut {
                    reason: format!(
                        "{} not met after {} ms",
                        if unsatisfied.is_empty() {
                            waits
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(" · ")
                        } else {
                            unsatisfied.join(" · ")
                        },
                        timeout.as_millis()
                    ),
                });
            }

            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Wait for the screen to be still, then take it. A child that exits during the wait is a
    /// final frame, not a failure.
    pub fn capture(&mut self, settle: Duration, timeout: Duration) -> CaptureOutcome {
        let started = Instant::now();
        loop {
            self.poll();
            if (self.quiet_for() >= settle && self.chunks > 0)
                || self.exit.is_some()
                || started.elapsed() >= timeout
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        let quiet = self.quiet_for();
        let mut snapshot = Snapshot::capture(self.parser.screen());
        if quiet < settle && self.exit.is_none() {
            snapshot.warnings.push(Warning::StillChanging {
                quiet_ms: quiet.as_millis() as u64,
            });
        }
        if let (Some(exit), true) = (&self.exit, snapshot.alternate_screen) {
            snapshot
                .warnings
                .push(Warning::ExitedInAlternateScreen { code: exit.code });
        }
        CaptureOutcome {
            snapshot,
            quiet_ms: quiet.as_millis() as u64,
            still_changing: quiet < settle,
        }
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<()> {
        let Some(writer) = self.writer.as_mut() else {
            bail!("{}: stdin is closed", self.name);
        };
        writer
            .write_all(bytes)
            .with_context(|| format!("{}: write to stdin", self.name))?;
        writer
            .flush()
            .with_context(|| format!("{}: flush stdin", self.name))
    }

    /// A lone ESC is ambiguous with the start of a sequence; give the app its escape timeout.
    pub fn send_escape(&mut self) -> Result<()> {
        self.send(b"\x1b")?;
        std::thread::sleep(Duration::from_millis(60));
        Ok(())
    }

    /// The screen as it is now, without waiting for it to settle. An assertion asks what the app
    /// currently shows; the wait before it is what makes that meaningful.
    pub fn snapshot(&mut self) -> Snapshot {
        self.poll();
        Snapshot::capture(self.parser.screen())
    }

    pub fn resize(&mut self, size: Size) -> Result<()> {
        self.master
            .resize(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("resize the pty")?;
        self.parser.set_size(size.rows, size.cols);
        Ok(())
    }

    pub fn kill(&mut self) {
        if self.exit.is_none() {
            if let Some(pid) = self.pid {
                crate::signals::terminate(pid);
            }
            let _ = self.child.kill();
            let started = Instant::now();
            while self.exit.is_none() && started.elapsed() < Duration::from_millis(500) {
                self.poll();
                std::thread::sleep(Duration::from_millis(5));
            }
            if self.exit.is_none() {
                self.exit = self.pending_exit.take();
            }
            if let Some(exit) = &mut self.exit {
                exit.terminated_by_tool = true;
            }
        }
        if let Some(pid) = self.pid {
            crate::signals::unregister(pid);
        }
        self.writer.take();
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        self.kill();
        // The reader thread ends on EOF once the child and writer are gone. Detaching it is the
        // only safe choice: a join here could hang behind a pty that never closes.
        let _ = self.reader.take();
    }
}

fn condition_holds(
    wait: &Wait,
    screen: &vt100::Screen,
    quiet: Duration,
    saw_output: bool,
    exited: bool,
) -> bool {
    match wait {
        Wait::Contains(needle) => screen.contents().contains(needle),
        Wait::Absent(needle) => !screen.contents().contains(needle),
        Wait::Idle(period) => saw_output && quiet >= *period,
        Wait::Exit => exited,
    }
}

fn exit_info(status: portable_pty::ExitStatus) -> ExitInfo {
    let signal = status.signal().map(str::to_owned);
    ExitInfo {
        code: if signal.is_some() {
            None
        } else {
            Some(status.exit_code() as i32)
        },
        signal,
        terminated_by_tool: false,
    }
}
