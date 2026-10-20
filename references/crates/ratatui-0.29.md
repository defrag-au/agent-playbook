# ratatui 0.29.0 (ratatui/ratatui)

## At a glance

- Use `ratatui::init()` and `ratatui::restore()`; `init` installs a panic hook that restores the terminal.
- Use the re-exported backend (`ratatui::crossterm`), never a separately pinned `crossterm`.
- `frame.area()` is the whole terminal; layout is `Layout::vertical`/`horizontal` over it.
- Filter events on `KeyEventKind::Press` or every key press is handled twice.
- This sheet answers init/draw/restore, layout, widgets and terminal hand-off.

## Cargo.toml

```toml
ratatui = "0.29"
```

The default features include the `crossterm` backend and `underline-color`. Ratatui
re-exports the backend it was built against as `ratatui::crossterm` (0.28.1 here). Use that
re-export instead of adding a separate `crossterm` dependency, otherwise two versions can end
up in the tree.

## Init, draw, restore

```rust
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

let mut terminal = ratatui::init();
loop {
    terminal.draw(|frame| {
        // frame.area() is the full terminal.
    })?;

    let Event::Key(key) = event::read()? else { continue };
    if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
        break;
    }
}
ratatui::restore();
```

`init` puts the terminal in raw mode and installs a panic hook that restores it. `restore` must
still be called on the normal exit path. For an editor hand-off, call `restore`, run the editor,
then call `init` again and replace the terminal value.

## Layout and widgets

```rust
let chunks = ratatui::layout::Layout::vertical([
    ratatui::layout::Constraint::Length(2),
    ratatui::layout::Constraint::Min(6),
    ratatui::layout::Constraint::Length(2),
])
.split(frame.area());
```

`Layout::areas::<N>()` returns `[Rect; N]`; `split()` returns a `Rects` slice. `Block::bordered()`
is the shorthand for all four borders.

## Table with selection

```rust
use ratatui::widgets::{Row, Table, TableState};

let mut state = TableState::default();
state.select(Some(0));

let table = Table::new(
    rows,
    [Constraint::Length(20), Constraint::Min(10)],
)
.header(Row::new(["Name", "Value"]))
.block(Block::bordered().title(" Rows "))
.row_highlight_style(Style::new().bg(Color::DarkGray));

frame.render_stateful_widget(table, area, &mut state);
```

`Table::new` takes the widths as its second argument in 0.29; `.widths()` exists for replacing
them. The header is a `Row`, not a `Vec<&str>`, in the builder.

## Headless rendering test

```rust
let backend = ratatui::backend::TestBackend::new(120, 40);
let mut terminal = ratatui::Terminal::new(backend)?;
terminal.draw(|frame| app.draw(frame))?;

let rendered = terminal
    .backend()
    .buffer()
    .content
    .iter()
    .map(|cell| cell.symbol())
    .collect::<String>();
assert!(rendered.contains("expected text"));
```

This exercises the real layout and widget code without a terminal, which is the useful check
for a TUI in CI.

## Gotchas

- `Style::bold()` and the other stylize helpers need `use ratatui::prelude::Stylize`. Without
  it, use `Style::new().add_modifier(Modifier::BOLD)`.
- `Frame` still has a lifetime in 0.29 (`Frame<'a>`), but `&mut Frame` in a render helper is
  enough; the lifetime is elided.
- `unicode-width` is pinned to `=0.2.0` by ratatui 0.29, so a newer patch version will not be
  selected even if the registry has one.
- `ratatui::init()` needs a real terminal. Check `std::io::stdout().is_terminal()` before
  starting if the binary can be piped.
- Crossterm can emit both press and release events depending on the platform. Ignore anything
  that is not `KeyEventKind::Press`.
- A table cell clips rather than wraps; put long paths and summaries in a `Paragraph` with
  `Wrap { trim: false }` instead.
