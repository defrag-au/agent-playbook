//! The process table, read through `libproc`.
//!
//! One snapshot and no sampling window: every figure is a lifetime total or derived from one, so
//! two runs a second apart describe the same history rather than two instants. That is what makes
//! the answer deterministic in the sense the contract means — no clock in the text, and no
//! window that has to be kept alive between calls.

use std::path::{Path, PathBuf};

use crate::contract::Fail;

/// One process, as much of it as the table needs.
#[derive(Debug, Clone)]
pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    /// The name the process registered, falling back to its command, then to nothing.
    pub name: String,
    /// `argv` joined with credential-shaped values replaced. Empty when the process would not
    /// say — another user's process, a kernel task, or a platform without the call.
    pub command: String,
    /// The nearest ancestor of the process's working directory (or, when that is nothing, its
    /// executable) holding `.git`.
    pub repo: Option<PathBuf>,
    /// Lifetime user + system time, in nanoseconds.
    pub cpu_ns: u64,
    /// Lifetime idle and interrupt wakeups — the battery-shaped half of "cost".
    pub wakeups: u64,
    pub footprint: u64,
    /// Process start, in seconds since the epoch.
    pub started: Option<u64>,
}

impl Proc {
    pub fn cpu_seconds(&self) -> f64 {
        self.cpu_ns as f64 / 1e9
    }

    /// How long it has existed, in seconds.
    pub fn age_seconds(&self, now: u64) -> Option<u64> {
        self.started.map(|started| now.saturating_sub(started))
    }

    /// Lifetime CPU as a percentage of the wall time it has existed. This is the figure that
    /// turns "25 % now" into "half a core for two and a half days".
    pub fn average_percent(&self, now: u64) -> Option<f64> {
        let age = self.age_seconds(now)? as f64;
        if age < 1.0 {
            return None;
        }
        Some(self.cpu_seconds() / age * 100.0)
    }
}

/// One thread, as the process table reports it.
#[derive(Debug, Clone)]
pub struct Thread {
    pub tid: u64,
    pub name: String,
    /// User + system time, in whatever unit the platform reports — only the ratio across a
    /// process's threads is used, so the unit never leaks into an answer.
    pub ticks: u64,
}

/// Everything one read of the table produced.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub procs: Vec<Proc>,
    /// The epoch second the snapshot was taken, for ages. Never printed.
    pub taken_at: u64,
    /// Processes the table listed but would not describe — permission, usually.
    pub unreadable: usize,
    /// Argument values replaced because the flag holding them is credential-shaped.
    pub redacted_arguments: usize,
}

/// Read the whole process table.
pub fn snapshot() -> Result<Snapshot, Fail> {
    imp::snapshot()
}

/// Read one process's threads.
pub fn threads(pid: i32) -> Result<Vec<Thread>, Fail> {
    imp::threads(pid)
}

/// The nearest ancestor holding `.git`, or `None` when the directory is inside no repository.
///
/// `.git` is stat-ed for and never opened — history is `at-recall`'s, and this tool does not
/// cross that line even to read a name.
pub fn repo_of(start: &Path) -> Option<PathBuf> {
    let mut here = start;
    loop {
        if here.join(".git").exists() {
            return Some(here.to_path_buf());
        }
        here = here.parent()?;
    }
}

/// Flags whose value is a credential. Matched on the name, not the value, because the value is
/// the thing that must never be printed.
fn credential_shaped(name: &str) -> bool {
    let lower = name.trim_start_matches('-').to_lowercase();
    const CONTAINS: &[&str] = &[
        "token",
        "secret",
        "password",
        "passwd",
        "passphrase",
        "credential",
    ];
    // Exact names rather than a `key` suffix: every Chromium helper passes
    // `--origin-trial-public-key`, and a rule that matches it redacts away the caveat's meaning.
    const EXACT: &[&str] = &[
        "key",
        "apikey",
        "api-key",
        "api_key",
        "private-key",
        "access-key",
        "secret-key",
        "signing-key",
        "auth",
        "bearer",
    ];

    CONTAINS.iter().any(|word| lower.contains(word)) || EXACT.contains(&lower.as_str())
}

/// Join `argv` for display, replacing the value of any credential-shaped flag.
///
/// Returns the line and how many values were replaced, so the caller can say so in a caveat
/// rather than silently shortening a command.
pub fn redact(argv: &[String]) -> (String, usize) {
    let mut out: Vec<String> = Vec::with_capacity(argv.len());
    let mut replaced = 0;
    let mut index = 0;

    while let Some(arg) = argv.get(index) {
        if arg.starts_with('-') {
            if let Some((name, _value)) = arg.split_once('=') {
                if credential_shaped(name) {
                    out.push(format!("{name}=<redacted>"));
                    replaced += 1;
                    index += 1;
                    continue;
                }
            }

            if credential_shaped(arg) {
                out.push(arg.clone());
                if argv.get(index + 1).is_some() {
                    out.push("<redacted>".to_string());
                    replaced += 1;
                    index += 2;
                    continue;
                }
            }
        }

        out.push(arg.clone());
        index += 1;
    }

    (out.join(" "), replaced)
}

/// The `KERN_PROCARGS2` buffer: a native-endian `int` argc, the executable path, NUL padding,
/// then `argc` NUL-terminated arguments, then the environment — which is never parsed.
///
/// Split out from the syscall so the layout has a test rather than only a machine.
pub fn parse_procargs(buffer: &[u8]) -> Option<Vec<String>> {
    let argc = i32::from_ne_bytes(buffer.get(0..4)?.try_into().ok()?);
    if !(0..=4096).contains(&argc) {
        return None;
    }
    let argc = argc as usize;

    let mut index = 4;
    while index < buffer.len() && buffer[index] != 0 {
        index += 1;
    }
    while index < buffer.len() && buffer[index] == 0 {
        index += 1;
    }

    let mut out = Vec::with_capacity(argc);
    for _ in 0..argc {
        let start = index;
        while index < buffer.len() && buffer[index] != 0 {
            index += 1;
        }
        if index > buffer.len() {
            break;
        }
        out.push(String::from_utf8_lossy(&buffer[start..index]).into_owned());
        index += 1;
    }
    Some(out)
}

/// A fixed-size C char array as a string, up to its first NUL.
#[cfg(target_os = "macos")]
fn c_string(bytes: &[i8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let bytes: Vec<u8> = bytes[..end].iter().map(|byte| *byte as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(target_os = "macos")]
mod imp {
    use std::os::raw::{c_int, c_void};
    use std::time::{SystemTime, UNIX_EPOCH};

    use libproc::pid_rusage::{pidrusage, RUsageInfoV2};
    use libproc::proc_pid::{listpidinfo, pidinfo, pidpath, ListThreads};
    use libproc::processes::{pids_by_type, ProcFilter};
    use libproc::task_info::TaskAllInfo;
    use libproc::thread_info::ThreadInfo;

    use super::{c_string, parse_procargs, redact, repo_of, Proc, Snapshot, Thread};
    use crate::contract::Fail;

    const CTL_KERN: c_int = 1;
    const KERN_ARGMAX: c_int = 8;
    const KERN_PROCARGS2: c_int = 49;
    const PROC_PIDVNODEPATHINFO: c_int = 9;

    #[repr(C)]
    struct MachTimebaseInfo {
        numer: u32,
        denom: u32,
    }

    /// `PATH_MAX` from `sys/syslimits.h`, the size of `vip_path` in `proc_vnodepathinfo`.
    const PATH_MAX: usize = 1024;

    // `proc_vnodepathinfo` from `<sys/proc_info.h>`. Reproduced field for field because the
    // kernel writes it by offset and `libproc` does not expose the type (its `pidcwd` is a
    // macOS stub). The size assertions below are the guard: a layout mistake fails the build
    // rather than letting the kernel write past a shorter struct.
    #[repr(C)]
    struct VinfoStat {
        vst_dev: u32,
        vst_mode: u16,
        vst_nlink: u16,
        vst_ino: u64,
        vst_uid: u32,
        vst_gid: u32,
        vst_atime: i64,
        vst_atimensec: i64,
        vst_mtime: i64,
        vst_mtimensec: i64,
        vst_ctime: i64,
        vst_ctimensec: i64,
        vst_birthtime: i64,
        vst_birthtimensec: i64,
        vst_size: i64,
        vst_blocks: i64,
        vst_blksize: i32,
        vst_flags: u32,
        vst_gen: u32,
        vst_rdev: u32,
        vst_qspare: [i64; 2],
    }

    #[repr(C)]
    struct Fsid {
        val: [i32; 2],
    }

    #[repr(C)]
    struct VnodeInfo {
        vi_stat: VinfoStat,
        vi_type: i32,
        vi_pad: i32,
        vi_fsid: Fsid,
    }

    #[repr(C)]
    struct VnodeInfoPath {
        vip_vi: VnodeInfo,
        vip_path: [u8; PATH_MAX],
    }

    #[repr(C)]
    struct ProcVnodePathInfo {
        pvi_cdir: VnodeInfoPath,
        pvi_rdir: VnodeInfoPath,
    }

    const _: () = {
        assert!(std::mem::size_of::<VnodeInfoPath>() == 152 + PATH_MAX);
        assert!(std::mem::size_of::<ProcVnodePathInfo>() == 2 * (152 + PATH_MAX));
    };

    // The one unsafe surface in the toolkit. `KERN_PROCARGS2` is the only way to read another
    // process's arguments — `proc_pidinfo` does not expose them — and it is the same call `ps`
    // makes. Both uses pass a correctly sized buffer and check the return value before reading.
    unsafe extern "C" {
        fn sysctl(
            name: *const c_int,
            namelen: u32,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> c_int;

        fn proc_pidinfo(
            pid: c_int,
            flavor: c_int,
            arg: u64,
            buffer: *mut c_void,
            buffersize: c_int,
        ) -> c_int;

        fn mach_timebase_info(info: *mut MachTimebaseInfo) -> c_int;
    }

    /// The rusage counters are in mach absolute units, not nanoseconds: on Apple Silicon the
    /// ratio is 125/3, so a raw read under-reports by ~41.7x. Verified against `ps -o time`
    /// for a long-running process, and identity on an Intel machine (numer == denom == 1).
    fn ticks_to_ns(ticks: u64) -> u64 {
        use std::sync::OnceLock;

        static TIMEBASE: OnceLock<(u64, u64)> = OnceLock::new();
        let (numer, denom) = *TIMEBASE.get_or_init(|| {
            let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
            let returned = unsafe { mach_timebase_info(&mut info) };
            if returned == 0 && info.denom != 0 {
                (u64::from(info.numer), u64::from(info.denom))
            } else {
                (1, 1)
            }
        });
        ticks.saturating_mul(numer) / denom
    }

    /// A process's working directory, through `PROC_PIDVNODEPATHINFO`.
    fn cwd(pid: i32) -> Option<std::path::PathBuf> {
        let mut info: ProcVnodePathInfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<ProcVnodePathInfo>() as c_int;
        let returned = unsafe {
            proc_pidinfo(
                pid,
                PROC_PIDVNODEPATHINFO,
                0,
                &mut info as *mut ProcVnodePathInfo as *mut c_void,
                size,
            )
        };
        if returned <= 0 {
            return None;
        }
        let path = &info.pvi_cdir.vip_path;
        let end = path.iter().position(|byte| *byte == 0)?;
        if end == 0 {
            return None;
        }
        Some(std::path::PathBuf::from(
            String::from_utf8_lossy(&path[..end]).into_owned(),
        ))
    }

    /// The kernel's argument buffer size, once per snapshot.
    fn argmax() -> Option<usize> {
        let mut mib: [c_int; 2] = [CTL_KERN, KERN_ARGMAX];
        let mut value: c_int = 0;
        let mut size = std::mem::size_of::<c_int>();
        let returned = unsafe {
            sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                &mut value as *mut c_int as *mut c_void,
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (returned == 0 && value > 0).then_some(value as usize)
    }

    fn argv(pid: i32, buffer: &mut Vec<u8>, argmax: usize) -> Option<Vec<String>> {
        buffer.clear();
        buffer.resize(argmax, 0);
        let mut size = argmax;
        let mut mib: [c_int; 3] = [CTL_KERN, KERN_PROCARGS2, pid];
        let returned = unsafe {
            sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                buffer.as_mut_ptr() as *mut c_void,
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if returned != 0 {
            return None;
        }
        buffer.truncate(size);
        parse_procargs(buffer)
    }

    pub fn snapshot() -> Result<Snapshot, Fail> {
        let pids = pids_by_type(ProcFilter::All)
            .map_err(|e| Fail::environment(format!("cannot list processes: {e}")))?;

        let taken_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or(0);

        let mut snapshot = Snapshot {
            taken_at,
            ..Snapshot::default()
        };
        let mut buffer = Vec::new();
        let argmax = argmax().unwrap_or(0);

        for raw in pids {
            let pid = raw as i32;
            let Ok(info) = pidinfo::<TaskAllInfo>(pid, 0) else {
                snapshot.unreadable += 1;
                continue;
            };

            let rusage = pidrusage::<RUsageInfoV2>(pid).ok();
            let cwd = cwd(pid);
            let exe = pidpath(pid).ok();
            // Attribution is the working directory first, because that is what an agent's
            // `cd` and a `direnv exec` both set. A binary inside a repository is the fallback,
            // which is what catches `target/debug/<tool>`. An app launched from Finder has `/`
            // as its directory and no repository anywhere, and stays in the caveat.
            let repo = cwd.as_deref().and_then(repo_of).or_else(|| {
                exe.as_deref()
                    .and_then(|path| repo_of(std::path::Path::new(path)))
            });
            let arguments = if argmax > 0 {
                argv(pid, &mut buffer, argmax)
            } else {
                None
            };

            let (command, replaced) = match arguments.as_deref() {
                Some(argv) if !argv.is_empty() => redact(argv),
                _ => (exe.clone().unwrap_or_default(), 0),
            };
            snapshot.redacted_arguments += replaced;

            let named = c_string(&info.pbsd.pbi_name);
            let name = if named.is_empty() {
                c_string(&info.pbsd.pbi_comm)
            } else {
                named
            };

            snapshot.procs.push(Proc {
                pid,
                ppid: info.pbsd.pbi_ppid as i32,
                name,
                command,
                repo,
                cpu_ns: ticks_to_ns(
                    rusage
                        .as_ref()
                        .map_or(0, |usage| usage.ri_user_time + usage.ri_system_time),
                ),
                wakeups: rusage.as_ref().map_or(0, |usage| {
                    usage.ri_interrupt_wkups + usage.ri_pkg_idle_wkups
                }),
                footprint: rusage
                    .as_ref()
                    .map_or(info.ptinfo.pti_resident_size, |usage| {
                        usage.ri_phys_footprint
                    }),
                started: Some(info.pbsd.pbi_start_tvsec),
            });
        }

        Ok(snapshot)
    }

    pub fn threads(pid: i32) -> Result<Vec<Thread>, Fail> {
        let info = pidinfo::<TaskAllInfo>(pid, 0)
            .map_err(|e| Fail::environment(format!("cannot read process {pid}: {e}")))?;
        let count = info.ptinfo.pti_threadnum.max(0) as usize;
        let ids = listpidinfo::<ListThreads>(pid, count)
            .map_err(|e| Fail::environment(format!("cannot list the threads of {pid}: {e}")))?;

        let mut threads = Vec::with_capacity(ids.len());
        for tid in ids {
            // A thread that exits between the listing and the read is not an error: the table is
            // a snapshot of a moving thing, and the count says so.
            let Ok(thread) = pidinfo::<ThreadInfo>(pid, tid) else {
                continue;
            };
            threads.push(Thread {
                tid,
                name: c_string(&thread.pth_name),
                ticks: thread.pth_user_time + thread.pth_system_time,
            });
        }

        if threads.is_empty() && count > 0 {
            return Err(Fail::environment(format!(
                "the platform would not describe any of the {count} threads of {pid}"
            )));
        }
        Ok(threads)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::{Snapshot, Thread};
    use crate::contract::Fail;

    pub fn snapshot() -> Result<Snapshot, Fail> {
        Err(Fail::environment(
            "at-cost reads macOS process state · this platform is not supported yet",
        ))
    }

    pub fn threads(_pid: i32) -> Result<Vec<Thread>, Fail> {
        Err(Fail::environment(
            "at-cost reads macOS process state · this platform is not supported yet",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    #[test]
    fn a_credential_flag_loses_its_value_and_the_count_says_so() {
        let (line, replaced) = redact(&argv(&["tool", "--token", "abc123", "--verbose"]));
        assert_eq!(line, "tool --token <redacted> --verbose");
        assert_eq!(replaced, 1);

        let (line, replaced) = redact(&argv(&["tool", "--api-key=abc123", "serve"]));
        assert_eq!(line, "tool --api-key=<redacted> serve");
        assert_eq!(replaced, 1);
    }

    #[test]
    fn a_plain_flag_is_not_mistaken_for_a_credential() {
        let (line, replaced) = redact(&argv(&["git", "log", "--author", "damo", "--oneline"]));
        assert_eq!(line, "git log --author damo --oneline");
        assert_eq!(replaced, 0);
    }

    #[test]
    fn a_public_key_flag_is_not_a_credential() {
        // Every Chromium helper carries this, so treating it as a secret would make the
        // caveat's count meaningless rather than careful.
        let (line, replaced) = redact(&argv(&[
            "chrome",
            "--origin-trial-public-key",
            "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE",
        ]));
        assert_eq!(
            line,
            "chrome --origin-trial-public-key MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE"
        );
        assert_eq!(replaced, 0);
    }

    #[test]
    fn procargs_skips_the_exec_path_and_its_padding() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&3i32.to_ne_bytes());
        buffer.extend_from_slice(b"/usr/bin/node\0\0\0");
        buffer.extend_from_slice(b"node\0script.js\0--port\0");
        buffer.extend_from_slice(b"PATH=x\0");
        assert_eq!(
            parse_procargs(&buffer).unwrap(),
            argv(&["node", "script.js", "--port"])
        );
    }

    #[test]
    fn a_truncated_procargs_buffer_is_refused_rather_than_guessed() {
        assert!(parse_procargs(&[]).is_none());
        assert!(parse_procargs(&[0, 0]).is_none());
        let mut buffer = (-1i32).to_ne_bytes().to_vec();
        buffer.extend_from_slice(b"/bin/sh\0");
        assert!(parse_procargs(&buffer).is_none());
    }

    #[test]
    fn average_percent_is_lifetime_cpu_over_lifetime_age() {
        let proc = Proc {
            pid: 1,
            ppid: 1,
            name: "x".to_string(),
            command: "x".to_string(),
            repo: None,
            cpu_ns: 3_600_000_000_000,
            wakeups: 0,
            footprint: 0,
            started: Some(1_000),
        };
        // One CPU-hour over ten hours is ten percent of a core.
        assert_eq!(proc.average_percent(1_000 + 36_000), Some(10.0));
    }
}
