//! Kill only the children this tool started, by the pid it was given.
//!
//! The slots are fixed and atomic because a signal handler may not allocate or lock. Sixteen
//! processes is far more than a scenario needs; a spawn beyond the cap still runs and is still
//! killed on the normal path, it just is not reached by the interrupt handler.

#[cfg(unix)]
mod imp {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Once;

    const MAX: usize = 16;
    static PIDS: [AtomicU32; MAX] = [const { AtomicU32::new(0) }; MAX];
    static INSTALL: Once = Once::new();

    pub fn install() {
        INSTALL.call_once(|| unsafe {
            libc::signal(libc::SIGINT, handle as *const () as libc::sighandler_t);
            libc::signal(libc::SIGTERM, handle as *const () as libc::sighandler_t);
        });
    }

    pub fn register(pid: u32) {
        for slot in &PIDS {
            if slot
                .compare_exchange(0, pid, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return;
            }
        }
    }

    pub fn unregister(pid: u32) {
        for slot in &PIDS {
            if slot.load(Ordering::SeqCst) == pid {
                slot.store(0, Ordering::SeqCst);
            }
        }
    }

    /// Terminate the child and anything it started. `forkpty` makes the child a session leader, so
    /// the negative pid reaches its process group; the direct signal covers a platform where it
    /// did not.
    pub fn terminate(pid: u32) {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }

    extern "C" fn handle(_: libc::c_int) {
        for slot in &PIDS {
            let pid = slot.load(Ordering::SeqCst);
            if pid != 0 {
                terminate(pid);
            }
        }
        unsafe { libc::_exit(130) }
    }
}

#[cfg(not(unix))]
mod imp {
    pub fn install() {}
    pub fn register(_: u32) {}
    pub fn unregister(_: u32) {}
    pub fn terminate(_: u32) {}
}

pub use imp::{install, register, terminate, unregister};
