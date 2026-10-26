# ratatui 0.30.2 (ratatui/ratatui)

## At a glance

- 0.30 split the crate into `ratatui-core`, `ratatui-widgets` and `ratatui-crossterm`. Depend
  on `ratatui` alone; it re-exports all three, so the paths you know still work.
- New: `ratatui::run(|terminal| …)` wraps `init()` and `restore()` around a closure.
- New: `Event::as_key_press_event()` filters key presses in one call. Releases and repeats return
  `None`.
- New: `rect.layout(&layout)` returns `[Rect; N]` directly.
- The re-exported backend is crossterm 0.29 (`ratatui::crossterm`), not 0.28.
- The minimum supported Rust is 1.88.
- This sheet answers: a polling dashboard (table, panes, keys), and testing it headless.

## Cargo.toml

```toml
# Only the crossterm backend. The defaults also pull in all-widgets (the calendar,
# which brings in `time`), macros, layout-cache and underline-color.
ratatui = { version = "0.30.2", default-features = false, features = ["crossterm"] }
```

Never add a separate `crossterm` dependency. Use `ratatui::crossterm`, or two versions can end
up in the tree.

## Key differences from 0.29

- `ratatui::run(f)` is new. It calls `init()`, runs `f(&mut DefaultTerminal)`, then calls
  `restore()`. `init` still installs a panic hook that restores the terminal.
- `event::read()?.as_key_press_event()` replaces `if let Event::Key(k) … if k.kind == Press`.
- `frame.area().layout(&Layout::vertical([...]))` gives an array, so it destructures with
  `let [a, b, c] = …`. `Layout::split` still exists.
- Widgets live in `ratatui-widgets` but are imported from `ratatui::widgets` as before. `Table`,
  `Row`, `Cell`, `TableState`, `Paragraph`, `Wrap`, `Block` and `List` keep their 0.29
  signatures. `Table::new(rows, widths)` still takes the widths as its second argument.

## A polling dashboard

```rust
use std::time::Duration;

use ratatui::crossterm::event::{self, KeyCode};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState};
use ratatui::{DefaultTerminal, Frame};

fn main() -> std::io::Result<()> {
    let mut state = TableState::default();
    ratatui::run(|terminal| event_loop(terminal, &mut state))
}

fn event_loop(terminal: &mut DefaultTerminal, state: &mut TableState) -> std::io::Result<()> {
    loop {
        // Drain worker updates here (mpsc::Receiver::try_recv) before drawing.
        terminal.draw(|frame| draw(frame, state))?;
        // Wait briefly for a key, so the screen redraws when no key comes.
        if event::poll(Duration::from_millis(200))? {
            if let Some(key) = event::read()?.as_key_press_event() {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Down => state.select_next(),
                    KeyCode::Up => state.select_previous(),
                    _ => {}
                }
            }
        }
    }
}

fn draw(frame: &mut Frame, state: &mut TableState) {
    let [header, table, logs] = frame.area().layout(&Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(6),
        Constraint::Fill(1),
    ]));
    frame.render_widget(Line::from(vec![Span::from(" stack ").reversed()]), header);

    let rows = vec![Row::new([Cell::from("api"), Cell::from("running").green()])];
    let widget = Table::new(rows, [Constraint::Length(12), Constraint::Fill(1)])
        .header(Row::new(["SERVICE", "STATE"]).style(Style::new().bold()))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ")
        .block(Block::bordered().title(" services "));
    frame.render_stateful_widget(widget, table, state);

    frame.render_widget(
        Paragraph::new("log lines").block(Block::bordered().title(" logs ")),
        logs,
    );
}
```

Keep runtime calls off this thread. A worker thread that sends snapshots over `mpsc` keeps the
key loop responsive when a subprocess is slow. `container-compose`'s `crates/top` is a worked
example.

## Headless rendering test

```rust
use ratatui::backend::TestBackend;
use ratatui::Terminal;

let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
terminal.draw(|frame| draw(frame, &mut state)).unwrap();
let buffer = terminal.backend().buffer();
let screen: String = (0..24)
    .map(|y| (0..120).map(|x| buffer[(x, y)].symbol()).collect::<String>() + "\n")
    .collect();
assert!(screen.contains("▶ api"));
```

Print `screen` with `cargo test -- --nocapture` to read the layout. For the real terminal, run the
binary in a sized tmux pane (`tmux new-session -d -x 130 -y 34 …`) and `tmux capture-pane -p`.

## Gotchas

- `.bold()`, `.green()`, `.dark_gray()` and `.reversed()` on a `Span`, `Line`, `Cell` or
  `Paragraph` need `use ratatui::style::Stylize`.
- `ratatui::init()` and `run()` need a real terminal. Check `std::io::stdout().is_terminal()`
  first and fail with a message, or a piped run garbles its output.
- A `Table` with no rows draws only its header. Render a `Paragraph` explaining why instead.
- Table cells and paragraph lines clip rather than wrap. Leave a log pane unwrapped if
  "one log line is one screen line" scroll arithmetic depends on it.
- `TableState::select(None)` hides the highlight. Re-select by key after the rows change, or the
  selection jumps to a different service.
