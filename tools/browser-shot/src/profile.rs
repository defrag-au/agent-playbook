//! Browser profile directories, and removing them once the browser has really gone.
//!
//! `headless_chrome` kills and reaps the browser's **main** process on drop, but Chrome's
//! children (zygotes, the GPU and network processes, renderers) outlive it for a moment and
//! keep writing into the profile: GPU caches, `Network Persistent State`, databases. A
//! `remove_dir_all` that runs straight after the kill races them, fails part-way, and leaves the
//! folder behind. So removal first waits for every process whose command line names the
//! profile (each child is started with `--user-data-dir=<profile>`), kills any that linger, and
//! then retries the delete briefly.
//!
//! On SIGINT/SIGTERM the signal handlers kill the browsers and call [`on_signal`], which wakes a
//! cleanup thread; that thread removes every registered directory and exits 130. A second
//! signal exits at once.

use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long the profile's processes get to exit on their own before they are killed.
const EXIT_GRACE: Duration = Duration::from_secs(3);
/// How long the processes get after SIGKILL.
const KILL_GRACE: Duration = Duration::from_secs(2);
/// How long a failing delete is retried.
const DELETE_RETRY: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(50);

struct Registered {
    path: PathBuf,
    /// A directory that only holds other directories: removed only when empty.
    empty_only: bool,
}

static REGISTERED: Mutex<Vec<Registered>> = Mutex::new(Vec::new());
static SIGNAL_PIPE: AtomicI32 = AtomicI32::new(-1);
static SIGNALLED: AtomicBool = AtomicBool::new(false);

/// A browser profile directory, removed when dropped unless kept. Drop it after the
/// `Browser` that used it.
pub struct ProfileDir {
    pub path: PathBuf,
    pub keep: bool,
}

impl ProfileDir {
    pub fn create(path: PathBuf, keep: bool) -> std::io::Result<Self> {
        fs::create_dir_all(&path)?;
        if !keep {
            register(&path, false);
        }
        Ok(Self { path, keep })
    }
}

impl Drop for ProfileDir {
    fn drop(&mut self) {
        if !self.keep {
            remove(&self.path);
            unregister(&self.path);
        }
    }
}

/// A directory that holds profile directories: removed on drop (and on a signal) when empty.
pub struct ParentDir {
    pub path: PathBuf,
}

impl ParentDir {
    pub fn create(path: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&path)?;
        register(&path, true);
        Ok(Self { path })
    }
}

impl Drop for ParentDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
        unregister(&self.path);
    }
}

fn register(path: &Path, empty_only: bool) {
    start_signal_cleanup();
    if let Ok(mut registered) = REGISTERED.lock() {
        registered.push(Registered {
            path: path.to_path_buf(),
            empty_only,
        });
    }
}

fn unregister(path: &Path) {
    if let Ok(mut registered) = REGISTERED.lock() {
        registered.retain(|entry| entry.path != path);
    }
}

/// Remove a profile directory once no process is using it. Failures are reported on stderr,
/// not returned: this runs on the way out.
pub fn remove(path: &Path) {
    wait_for_processes(path);
    let deadline = Instant::now() + DELETE_RETRY;
    loop {
        let error = match fs::remove_dir_all(path) {
            Ok(()) => return,
            Err(error) if error.kind() == ErrorKind::NotFound => return,
            Err(error) => error,
        };
        if Instant::now() >= deadline {
            eprintln!(
                "browser-shot: could not remove profile {}: {error}",
                path.display()
            );
            return;
        }
        std::thread::sleep(POLL);
    }
}

/// Wait for the processes whose command line names `path` to exit; kill the stragglers.
fn wait_for_processes(path: &Path) {
    let start = Instant::now();
    let mut killed = false;
    loop {
        let pids = processes_using(path);
        if pids.is_empty() {
            return;
        }
        let elapsed = start.elapsed();
        if !killed && elapsed >= EXIT_GRACE {
            kill(&pids);
            killed = true;
        }
        if elapsed >= EXIT_GRACE + KILL_GRACE {
            return;
        }
        std::thread::sleep(POLL);
    }
}

/// Processes, other than this one, with an argument that contains `path`.
#[cfg(target_os = "linux")]
fn processes_using(path: &Path) -> Vec<i32> {
    let Some(needle) = path.to_str() else {
        return Vec::new();
    };
    let own = std::process::id() as i32;
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|&pid| pid != own)
        .filter(|pid| {
            fs::read(format!("/proc/{pid}/cmdline"))
                .map(|cmdline| {
                    cmdline
                        .split(|&byte| byte == 0)
                        .any(|arg| names(arg, needle))
                })
                .unwrap_or(false)
        })
        .collect()
}

/// Without `/proc`, rely on the delete retry alone.
#[cfg(not(target_os = "linux"))]
fn processes_using(_path: &Path) -> Vec<i32> {
    Vec::new()
}

/// Whether a command-line argument names the profile `needle` (or a path inside it), and not
/// merely a sibling that shares its prefix (`browser-shot-12` is not `browser-shot-123`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn names(arg: &[u8], needle: &str) -> bool {
    let needle = needle.as_bytes();
    arg.windows(needle.len()).enumerate().any(|(at, window)| {
        window == needle && matches!(arg.get(at + needle.len()), None | Some(b'/') | Some(b'"'))
    })
}

#[cfg(unix)]
fn kill(pids: &[i32]) {
    for &pid in pids {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn kill(_pids: &[i32]) {}

/// Start the thread that cleans up after a signal. Idempotent.
#[cfg(unix)]
fn start_signal_cleanup() {
    use std::os::fd::FromRawFd;

    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let mut fds = [0 as libc::c_int; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return;
    }
    let mut reader = unsafe { fs::File::from_raw_fd(fds[0]) };
    SIGNAL_PIPE.store(fds[1], Ordering::SeqCst);
    let spawned = std::thread::Builder::new()
        .name("profile-cleanup".to_owned())
        .spawn(move || {
            let mut byte = [0u8; 1];
            if reader.read(&mut byte).unwrap_or(0) == 0 {
                return;
            }
            let entries = REGISTERED
                .lock()
                .map(|registered| {
                    registered
                        .iter()
                        .map(|entry| (entry.path.clone(), entry.empty_only))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            // Profiles first, then the directories that held them.
            for (path, _) in entries.iter().filter(|(_, empty_only)| !empty_only) {
                remove(path);
            }
            for (path, _) in entries.iter().filter(|(_, empty_only)| *empty_only) {
                let _ = fs::remove_dir(path);
            }
            unsafe { libc::_exit(130) };
        });
    if spawned.is_err() {
        SIGNAL_PIPE.store(-1, Ordering::SeqCst);
    }
}

#[cfg(not(unix))]
fn start_signal_cleanup() {}

/// Called from a signal handler after the browsers have been sent SIGTERM. Async-signal-safe.
/// Returns true when the cleanup thread has taken over (it exits the process); false when the
/// handler should `_exit` itself: no cleanup thread, or a second signal.
#[cfg(unix)]
pub fn on_signal() -> bool {
    let fd = SIGNAL_PIPE.load(Ordering::SeqCst);
    if fd < 0 || SIGNALLED.swap(true, Ordering::SeqCst) {
        return false;
    }
    let byte = 1u8;
    unsafe { libc::write(fd, &byte as *const u8 as *const libc::c_void, 1) == 1 }
}

/// Exit with `code`, unless a signal's cleanup is under way: then leave the exit (130) to it.
pub fn exit(code: i32) -> ! {
    if SIGNALLED.load(Ordering::SeqCst) {
        loop {
            std::thread::park();
        }
    }
    std::process::exit(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_argument_names_the_profile_or_a_path_inside_it() {
        let profile = "/w/.tmp/browser-shot-12";
        assert!(names(b"--user-data-dir=/w/.tmp/browser-shot-12", profile));
        assert!(names(
            b"--database=/w/.tmp/browser-shot-12/Crashpad",
            profile
        ));
        assert!(!names(b"--user-data-dir=/w/.tmp/browser-shot-123", profile));
        assert!(!names(b"--type=renderer", profile));
    }

    #[test]
    fn remove_deletes_a_populated_directory_and_tolerates_a_missing_one() {
        let dir = std::env::temp_dir().join(format!("profile-test-{}", std::process::id()));
        fs::create_dir_all(dir.join("Default/Cache")).unwrap();
        fs::write(dir.join("Default/Cache/data"), b"x").unwrap();
        remove(&dir);
        assert!(!dir.exists());
        remove(&dir);
        assert!(!dir.exists());
    }

    #[test]
    fn a_dropped_profile_is_removed_unless_kept() {
        let base = std::env::temp_dir().join(format!("profile-drop-{}", std::process::id()));
        let removed = base.join("removed");
        let kept = base.join("kept");
        {
            let _removed = ProfileDir::create(removed.clone(), false).unwrap();
            let _kept = ProfileDir::create(kept.clone(), true).unwrap();
            fs::write(removed.join("file"), b"x").unwrap();
        }
        assert!(!removed.exists());
        assert!(kept.exists());
        fs::remove_dir_all(&base).unwrap();
    }
}
