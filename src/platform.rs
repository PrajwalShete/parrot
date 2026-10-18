//! Terminal plumbing: raw key input with timeouts, restoring the terminal on exit, opening URLs.

use std::time::Duration;

/// Raw keyboard input (no echo, no line buffering, Ctrl+C arrives as a key) while alive.
pub struct RawInput {
    #[cfg(unix)]
    saved: libc::termios,
}

impl RawInput {
    pub fn enable() -> Option<Self> {
        #[cfg(unix)]
        unsafe {
            if libc::isatty(libc::STDIN_FILENO) != 1 {
                return None;
            }
            let mut saved: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut saved) != 0 {
                return None;
            }
            SAVED.get_or_init(|| saved);
            let mut raw = saved;
            raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
                return None;
            }
            Some(Self { saved })
        }
        #[cfg(not(unix))]
        None
    }

    /// True when a key is waiting to be read.
    pub fn wait(timeout: Duration) -> bool {
        #[cfg(unix)]
        unsafe {
            // select(), because macOS poll() can't wait on terminals.
            let mut set: libc::fd_set = std::mem::zeroed();
            libc::FD_ZERO(&mut set);
            libc::FD_SET(libc::STDIN_FILENO, &mut set);
            let mut tv = libc::timeval {
                tv_sec: timeout.as_secs() as _,
                tv_usec: timeout.subsec_micros() as _,
            };
            libc::select(
                libc::STDIN_FILENO + 1,
                &mut set,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut tv,
            ) > 0
        }
        #[cfg(not(unix))]
        {
            std::thread::sleep(timeout);
            false
        }
    }
}

impl Drop for RawInput {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.saved);
        }
    }
}

#[cfg(unix)]
static SAVED: std::sync::OnceLock<libc::termios> = std::sync::OnceLock::new();

/// Restores the terminal (mode, cursor, main screen) if we panic or are killed mid-animation.
pub fn guard_terminal() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
    #[cfg(unix)]
    unsafe {
        extern "C" fn on_signal(sig: libc::c_int) {
            restore();
            unsafe {
                libc::signal(sig, libc::SIG_DFL);
                libc::raise(sig);
            }
        }
        for sig in [libc::SIGTERM, libc::SIGHUP] {
            libc::signal(sig, on_signal as *const () as libc::sighandler_t);
        }
    }
}

fn restore() {
    use std::io::Write;
    let _ = std::io::stdout().write_all(b"\x1b[?2026l\x1b[0m\x1b[?1049l\x1b[?25h");
    let _ = std::io::stdout().flush();
    #[cfg(unix)]
    if let Some(saved) = SAVED.get() {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, saved);
        }
    }
}

/// Drops keys typed ahead (so an Enter pressed during an animation doesn't answer the next question).
pub fn flush_input() {
    #[cfg(unix)]
    unsafe {
        libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
    }
}

pub fn open_url(url: &str) -> bool {
    open::that_detached(url).is_ok()
}
