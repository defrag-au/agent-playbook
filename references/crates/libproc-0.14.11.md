# libproc 0.14.11 (andrewdavidmackenzie/libproc-rs)

## At a glance

- Wraps `proc_pidinfo`, `proc_pid_rusage`, `proc_listpids` and friends. MIT, `rust-version = 1.72`.
- **`listpids` is deprecated** — use `processes::pids_by_type(ProcFilter::All)`.
- **`pidcwd` is a stub on macOS** and always returns `Err`. `PROC_PIDVNODEPATHINFO` is the only
  route to a working directory, and this crate does not wrap it — declare it yourself.
- **`RUsageInfoV2` carries no energy field.** `ri_energy_nj` appears nowhere in the crate; the
  v2 struct stops at `ri_diskio_byteswritten`. Wakeups and CPU time are the usable signals.
- **There is no `argv` support.** `KERN_PROCARGS2` is a raw `sysctl`, not something this wraps.
- This sheet answers: *given a pid, what is its parent, cwd, command, lifetime CPU, wakeups and
  per-thread time, without spawning `ps`?*

## Cargo.toml

```toml
# macOS-only surface is cfg-gated; the crate still compiles on Linux with stubs.
[target.'cfg(target_os = "macos")'.dependencies]
libproc = "0.14.11"
```

Build deps: `bindgen` and `cc` run in its build script, so it needs clang on the build host.

## Enumerate, then describe

```rust
use libproc::processes::{ProcFilter, pids_by_type};
use libproc::proc_pid::pidinfo;
use libproc::task_info::TaskAllInfo;

for raw in pids_by_type(ProcFilter::All)? {
    let pid = raw as i32;
    let Ok(info) = pidinfo::<TaskAllInfo>(pid, 0) else { continue };
    let ppid = info.pbsd.pbi_ppid;
    let started = info.pbsd.pbi_start_tvsec;          // epoch seconds
    let threads = info.ptinfo.pti_threadnum;          // i32, can be 0
    let _resident = info.ptinfo.pti_resident_size;
}
```

`pidinfo::<T>(pid, arg)` passes `arg` straight to `proc_pidinfo`. `0` for most flavors.

## Lifetime CPU, wakeups and footprint

```rust
use libproc::pid_rusage::{pidrusage, RUsageInfoV2};

let usage = pidrusage::<RUsageInfoV2>(pid)?;
let _user = usage.ri_user_time;                 // NOT nanoseconds — see Gotchas
let _wakeups = usage.ri_interrupt_wkups + usage.ri_pkg_idle_wkups;
let _footprint = usage.ri_phys_footprint;
```

## Per-thread time

`ListThreads` yields `u64` thread ids, and `ThreadInfo` is `proc_threadinfo`:

```rust
use libproc::proc_pid::{listpidinfo, pidinfo, ListThreads};
use libproc::task_info::TaskAllInfo;
use libproc::thread_info::ThreadInfo;

let info = pidinfo::<TaskAllInfo>(pid, 0)?;
let ids = listpidinfo::<ListThreads>(pid, info.ptinfo.pti_threadnum.max(0) as usize)?;
for tid in ids {
    let thread = pidinfo::<ThreadInfo>(pid, tid)?;   // tid goes in `arg`, not `pid`
    let _name = thread.pth_name;                      // [i8; 64], NUL-terminated
    let _ticks = thread.pth_user_time + thread.pth_system_time;
}
```

The thread counters are in the same unit as the rusage ones, so a **ratio** across a process's
threads is safe without knowing the unit. Prefer the ratio; an absolute figure needs the
timebase (below).

## Gotchas

- **The rusage time unit is mach absolute, not nanoseconds.** On Apple Silicon the ratio is
  `125/3`, so an unconverted read under-reports by ~41.7x — a process `ps` reports with 239
  minutes shows 5m45s. Convert with `mach_timebase_info` (`ns = ticks * numer / denom`; identity
  on Intel). Verified against `ps -o time`.
- **`PROC_PIDVNODEPATHINFO` is a `#[repr(C)]` struct you have to declare.** `proc_vnodepathinfo`
  is `{ pvi_cdir, pvi_rdir }` of `vnode_info_path`, which is a 152-byte `vnode_info` followed by
  `vip_path: [u8; 1024]`. Assert `size_of` at compile time: the kernel writes by offset, so a
  short struct is a stack overwrite, not an error.
- **`pidinfo::<TaskAllInfo>` fails without permission** for other users' processes and for many
  kernel tasks. Count the failures — on a normal Mac a third of the table is unreadable.
- **`pth_name` is often empty**, and `pbi_name` is empty for processes that registered no name.
- **Energy is not here.** `proc_pid_rusage` has it in `rusage_info_v2`'s successor under
  `ri_energy_nj`, but this crate's structs do not model it, and a hand-written struct of the
  wrong size is a memory-safety bug rather than a compile error.
