//! A deliberately small TUI for `tui-shot`'s own tests.
//!
//! It uses no framework: raw mode, the alternate screen, a box drawn to the current size, and a
//! handful of keys. That is enough to exercise the parts of `tui-shot` that matter — the PTY, the
//! emulator, resize, styles and clean exit — without a test dependency of its own.

use std::io::{Read, Write};

fn main() {
    let screen = Screen::enter();
    let mut state = State::default();
    let mut input = std::io::stdin();

    loop {
        let size = size();
        if state.dirty || size != state.size {
            state.size = size;
            state.dirty = false;
            let frame = draw(&state);
            let mut out = std::io::stdout();
            let _ = write!(out, "{frame}");
            let _ = out.flush();
        }
        if state.quit {
            break;
        }

        let mut poll = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, 20) };
        if ready <= 0 {
            continue;
        }

        let mut buffer = [0u8; 256];
        let read = match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        for byte in &buffer[..read] {
            state.key(*byte);
        }
    }

    drop(screen);
}

#[derive(Default)]
struct State {
    compose: bool,
    draft: String,
    sent: Option<String>,
    permission: bool,
    quit: bool,
    dirty: bool,
    size: (u16, u16),
}

impl State {
    fn key(&mut self, byte: u8) {
        self.dirty = true;
        if self.compose {
            match byte {
                b'\r' | b'\n' => {
                    if !self.draft.is_empty() {
                        self.sent = Some(std::mem::take(&mut self.draft));
                    }
                    self.compose = false;
                }
                0x1b => self.compose = false,
                0x7f | 0x08 => {
                    self.draft.pop();
                }
                0x03 => self.quit = true,
                0x20..=0x7e => self.draft.push(byte as char),
                _ => {}
            }
            return;
        }

        match byte {
            b'q' | 0x03 => self.quit = true,
            b'i' => self.compose = true,
            b'1' => self.permission = !self.permission,
            _ => {}
        }
    }
}

struct Screen {
    original: libc::termios,
}

impl Screen {
    fn enter() -> Self {
        let original = unsafe {
            let mut original: libc::termios = std::mem::zeroed();
            libc::tcgetattr(0, &mut original);
            let mut raw = original;
            libc::cfmakeraw(&mut raw);
            libc::tcsetattr(0, libc::TCSANOW, &raw);
            original
        };
        let mut out = std::io::stdout();
        let _ = write!(out, "\x1b[?1049h\x1b[?25l");
        let _ = out.flush();
        Self { original }
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.original);
        }
        let mut out = std::io::stdout();
        let _ = write!(out, "\x1b[0m\x1b[?25h\x1b[?1049l");
        let _ = out.flush();
    }
}

fn size() -> (u16, u16) {
    let mut window: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(0, libc::TIOCGWINSZ, &mut window) } == 0;
    if ok && window.ws_col > 0 && window.ws_row > 0 {
        (window.ws_col, window.ws_row)
    } else {
        (80, 24)
    }
}

fn draw(state: &State) -> String {
    let (cols, rows) = state.size;
    let cols = cols.max(20) as usize;
    let rows = rows.max(6) as usize;
    let inner = cols.saturating_sub(2);
    let mut lines = Vec::with_capacity(rows);

    let title = " tui-shot demo ";
    let border = if title.len() + 2 <= cols {
        format!(
            "┌{}{}┐",
            title,
            "─".repeat(cols.saturating_sub(2 + title.len()))
        )
    } else {
        format!("┌{}┐", "─".repeat(inner))
    };
    lines.push(border);

    let status = match &state.sent {
        Some(text) => format!("sent: {text}"),
        None => "sent: nothing yet".to_owned(),
    };
    let draft = if state.compose {
        format!("draft: {}_", state.draft)
    } else {
        "draft: (press i)".to_owned()
    };

    let mut content = vec!["hello from tui-shot".to_owned(), status, draft];
    if state.permission {
        content.push(String::new());
        content.push(" 1 deploy   2 cancel ".to_owned());
    }
    content.push("i compose · 1 permission · q quit".to_owned());

    let body = rows.saturating_sub(2);
    for row in 0..body {
        let text = content.get(row).map_or("", String::as_str);
        let text = fit(text, inner);
        let padding = inner.saturating_sub(text.chars().count());
        if state.permission && text.contains("deploy") {
            lines.push(format!("│\x1b[7m{}{}\x1b[0m│", text, " ".repeat(padding)));
        } else {
            lines.push(format!("│{}{}│", text, " ".repeat(padding)));
        }
    }
    lines.push(format!("└{}┘", "─".repeat(inner)));

    format!("\x1b[H{}", lines.join("\r\n"))
}

fn fit(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    text.chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}
