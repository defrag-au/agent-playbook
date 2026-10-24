# headless_chrome 1.0.22 (rust-headless-chrome/rust-headless-chrome)

## At a glance

- Synchronous, thread-based CDP client. No tokio; the CLI that uses it can be plain `main`.
- `Browser::new(LaunchOptions)` launches a browser **and owns the child process**;
  `TemporaryProcess`'s `Drop` kills it (`src/browser/process.rs:70`). `Browser::connect(ws_url)`
  attaches to one you did not launch, and then cleanup is yours.
- Launch picks a free port in 8000–9000 (or `port: Some(n)`), adds `--headless` when
  `headless: true`, `--enable-automation`, and `--disable-gpu` unless `enable_gpu`.
- `Tab::call_method` accepts every generated CDP command, so a missing helper is never a wall.
- This sheet answers: launch and own a browser, set a true viewport, click a canvas, capture a
  PNG, collect console errors, and use raw CDP commands.
- Working example: `tools/browser-shot/src/main.rs` in this repository.

## Cargo.toml

```toml
headless_chrome = { version = "1.0.22", default-features = false, features = ["offline"] }
```

`offline` generates the protocol from the bundled JSON, so no build-time download. Default
features are exactly `["offline"]`. Do not enable `fetch`: it pulls `ureq` and downloads a
browser. No TLS feature is needed for `ws://127.0.0.1`; if one is, use
`rustls-tls-webpki-roots`, not `native-tls` (OpenSSL).

## Launch a browser and own it

```rust
use std::time::Duration;
use headless_chrome::{Browser, LaunchOptions};

let options = LaunchOptions::default_builder()
    .path(Some("/Applications/Brave Browser.app/Contents/MacOS/Brave Browser".into()))
    .user_data_dir(Some(profile.clone()))
    .headless(true)
    .ignore_certificate_errors(false)
    .idle_browser_timeout(Duration::from_secs(300))
    .args(vec![
        "--use-gl=swiftshader".as_ref(),
        "--enable-unsafe-swiftshader".as_ref(),
        "--hide-scrollbars".as_ref(),
    ])
    .build()?;

let browser = Browser::new(options)?;
let tab = browser.new_tab()?;
// Dropping `browser` closes the browser and kills the child.
```

Declare a profile guard **before** the browser so it removes the directory after the child has
been reaped. `idle_browser_timeout` defaults to 30 s and is a real trap on slow wasm boots.
`headless(true)` adds `--headless`; do not also pass `--headless=new` unless you set
`headless(false)`. `ignore_certificate_errors` defaults to **true** — set it false unless a
self-signed dev server needs it.

## Give the page a true viewport

```rust
use headless_chrome::protocol::cdp::Emulation;

tab.call_method(Emulation::SetDeviceMetricsOverride {
    width: 390,
    height: 844,
    device_scale_factor: 1.0,
    mobile: true,
    scale: None,
    screen_width: None,
    screen_height: None,
    position_x: None,
    position_y: None,
    dont_set_visible_size: None,
    screen_orientation: None,
    viewport: None,
    display_feature: None,
    device_posture: None,
})?;
```

`width` and `height` are `u32` in the generated Rust, not `i64`. The struct has no `Default`;
all fourteen fields are required. This is what makes a 390 px shot a 390 px layout instead of a
620 px layout cropped.

## Navigate, settle, capture

```rust
use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;

tab.navigate_to(url)?.wait_until_navigated()?;
let png = tab.capture_screenshot(CaptureScreenshotFormatOption::Png, None, None, true)?;
```

`capture_screenshot` takes `(format, quality, clip, from_surface)` in 1.0.x. Three-argument
examples from 0.x will not compile. `wait_until_navigated` is document load, not "the wasm app
has drawn"; poll captures until two in a row are byte-identical. `tab.get_title()` and
`tab.get_url()` are available for the report.

## Move and click, including a canvas

```rust
use headless_chrome::browser::tab::point::Point;

tab.move_mouse_to_point(Point { x: 100.0, y: 30.0 })?; // mouseMoved only
tab.click_point(Point { x: 40.0, y: 135.0 })?;        // move, then press + release
```

`click_point` moves the pointer first, which is the trick egui needs: a press with no prior
move is ignored because `hovered()` was never true. Coordinates are CSS pixels in the emulated
viewport; at device scale factor 1 they are also device pixels.

## Collect console errors and exceptions

```rust
use std::sync::{Arc, Mutex};
use headless_chrome::protocol::cdp::types::Event;

tab.enable_log()?.enable_runtime()?;
let lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
let sink = Arc::clone(&lines);

tab.add_event_listener(Arc::new(move |event: &Event| match event {
    Event::RuntimeConsoleAPICalled(event) => {
        // event.params.Type is Log/Info/Warning/Error/...
        // event.params.args: Vec<Runtime::RemoteObject>
    }
    Event::LogEntryAdded(event) => {
        // event.params.entry.level / .source / .text
    }
    Event::RuntimeExceptionThrown(event) => {
        // event.params.exception_details.text, .exception.description
    }
    _ => {}
}))?;
```

`console.error` and `console.warn` arrive as `Runtime.consoleAPICalled`, not `Log.entryAdded`.
`Log.entryAdded` is where browser and rendering messages land, including the headless
SwiftShader `GPU stall due to ReadPixels` warning, which is documented noise. A `RemoteObject`
exposes `description` first (best for objects) and `value` as `serde_json::Value` (the dynamic
external-API case the typed-JSON rule exempts).

## Any CDP command the helpers do not cover

```rust
use headless_chrome::protocol::cdp::Input;

tab.call_method(Input::DispatchKeyEvent { /* generated fields */ })?;
```

Commands are generated enums: `Emulation::SetDeviceMetricsOverride { .. }`,
`Page::CaptureScreenshot { .. }`. With `offline` enabled, the generated source is at
`target/<profile>/build/headless_chrome-*/out/protocol.rs` — grep it for exact field names and
types rather than guessing.

## Gotchas

- Launch discovers the WebSocket URL by **scraping the browser's stderr**. It retries with a
  new port up to ten times, but non-English output defeats it. For a robust launcher, spawn the
  browser yourself with `--remote-debugging-port=0`, poll `DevToolsActivePort` in the profile
  directory (port on line 1, `/devtools/browser/...` on line 2), then
  `Browser::connect("ws://127.0.0.1:<port><path>")`. Cleanup is yours in that mode.
- **`Emulation.setTouchEmulationEnabled` has the wrong generated parameters.**
  `auto_generate_cdp` 0.4.6 swaps them with the deprecated `Page.setTouchEmulationEnabled`:
  the `Emulation` struct carries `configuration`, and the modern `maxTouchPoints` lands on the
  `Page` struct. Send the correct shape yourself:

  ```rust
  use headless_chrome::protocol::cdp::types::Method;

  #[derive(Debug, serde::Serialize)]
  struct SetTouchEmulation {
      enabled: bool,
      #[serde(rename = "maxTouchPoints", skip_serializing_if = "Option::is_none")]
      max_touch_points: Option<u32>,
  }
  #[derive(Debug, serde::Deserialize)]
  struct Empty {}
  impl Method for SetTouchEmulation {
      const NAME: &'static str = "Emulation.setTouchEmulationEnabled";
      type ReturnObject = Empty;
  }
  tab.call_method(SetTouchEmulation { enabled: true, max_touch_points: Some(5) })?;
  ```

  Then verify with `navigator.maxTouchPoints`; a call that silently did nothing is the failure
  this check catches.
- **A fractional element clip is floored before the DPR is applied.** A 389.99997 CSS px box at
  DPR 3 captures 1167 px, not 1170. Expand the clip to whole CSS pixels before calling
  `Page.captureScreenshot`:

  ```rust
  let clip = Page::Viewport {
      x: measured.x.floor(),
      y: measured.y.floor(),
      width: (measured.x + measured.width).ceil() - measured.x.floor(),
      height: (measured.y + measured.height).ceil() - measured.y.floor(),
      scale: measured.scale,
  };
  ```

  For an element below the fold, also set `capture_beyond_viewport: Some(true)`.
- **`Element::call_js_fn` uses `return_by_value: false`.** Return a primitive (a joined string,
  a number) and parse it; an array comes back as an object reference with `value: None`.
- **Every `call_method` can wait up to 100 ms before it is sent.** The websocket reader holds
  the connection mutex while blocked in `read()`, and the socket has a 100 ms read timeout
  (`src/browser/transport/web_socket_connection.rs`), so the next command waits for that read
  to time out. On a 60 Hz page a round trip is about six frames. For frame-relative input,
  schedule the hold from a counter read **before** the event so the two round trips cancel;
  measuring from an event read after the dispatch stacks them and every hold overshoots.
- **`--headless` is the new headless.** `headless(true)` adds `--headless`; pass
  `--headless=new` yourself only with `headless(false)`. For a headed run set `enable_gpu(true)`
  or the crate adds `--disable-gpu` and the window renders in software.
- A persistent `user_data_dir` is never deleted by the crate; only the temp directory it
  creates itself is.
- `--enable-automation` is a default argument. Harmless for screenshots; visible to bot checks.
- Extensions are supported by `LaunchOptions::extensions`, but the crate documents them as
  headful-only. Wallet work needs a headful launch with a persistent profile.
- The first build generates roughly 60k lines of CDP code and is much slower than the rest of
  the workspace; later builds are cached.
