# ureq 3.4.2 (algesten/ureq)

Researched 2026-10-19 from crates.io and docs.rs, while adding a native-only HTTP
client to `solada-ledger`'s macroquad frontend.

## At a glance

- 3.x returns `http::Response<Body>`; `into_string()` and `into_json()` are gone.
- `json` is **not** a default feature — defaults are `rustls` and `gzip`. Read a string, or enable it.
- Timeouts live on the `Agent` config builder, not on the per-request builder.
- `Error::Status(code, response)` from 2.x is `Error::StatusCode(code)`.
- `call()` blocks; put it on a thread and drain an `mpsc` on the frame thread.
- This sheet answers a native-only typed HTTP client and its migration traps.

## Cargo.toml

```toml
# Default features are rustls + gzip. `json` is NOT a default in 3.x.
ureq = "3.4.2"

# In a crate that also builds for wasm, keep it off that target:
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ureq = { workspace = true }
```

Default `rustls` uses `ring` and Mozilla's `webpki-roots` (static, not the OS
store). `native-tls` exists but is never enabled by default.

## Key differences from ureq 2.x (the version you probably remember)

- `response.into_string()` is gone. The response is `http::Response<Body>`:
  `response.body_mut().read_to_string()?`.
- `agent.get(url).call()?` is still the request entry point, but an `Agent` is
  built from a config builder:
  `Agent::config_builder().timeout_global(Some(d)).build().into()`.
- `ureq::json::<T>()` / `into_json()` are gone. Enable the `json` feature for
  `send_json` / `body_mut().read_json::<T>()`, or read a string and use
  `serde_json` yourself (what this repo does).
- Timeouts are set on the agent (`timeout_global`, plus per-phase variants), not
  on each request builder.
- `ureq::Error::Status(code, response)` in 2.x is `ureq::Error::StatusCode(code)`
  in 3.x.
- The `tls` feature from 2.x is now `rustls` (or `native-tls`).

## GET a JSON document as a typed value

```rust
use std::time::Duration;

fn fetch_json<T: serde::de::DeserializeOwned>(
    agent: &ureq::Agent,
    url: &str,
) -> Result<T, String> {
    let body = agent
        .get(url)
        .call()
        .map_err(|error| format!("fetch {url}: {error}"))?
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("read {url}: {error}"))?;
    serde_json::from_str(&body).map_err(|error| format!("parse {url}: {error}"))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into()
}
```

## Blocking is the point — and the trap

`call()` blocks. In a game/render loop, do the request on a `std::thread` and
send the result down an `mpsc` channel; drain with `try_recv()` on the frame
thread. Never call it from a macroquad coroutine that is polled on the render
thread if the request can be slow.

```rust
use std::sync::mpsc;
use std::time::Duration;

fn spawn_follower<T>(url: String, tx: mpsc::Sender<Result<T, String>>)
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    std::thread::spawn(move || {
        let agent = agent();
        loop {
            if tx.send(fetch_json(&agent, &url)).is_err() {
                break; // receiver dropped: the app is gone
            }
            std::thread::sleep(Duration::from_secs(10));
        }
    });
}
```

## Gotchas

- **Proxy environment variables are honoured automatically.** `ALL_PROXY`,
  `HTTPS_PROXY`, `HTTP_PROXY` (and lowercase) are read when the default agent is
  created; `NO_PROXY` supports exact hosts, `*.example.com`, `.example.com` and
  `*`. A sandbox that routes traffic through a proxy therefore works without
  code changes — but a request can appear to reach a different host than the one
  in the URL if the proxy is misconfigured.
- **`http_status_as_error` defaults to true.** A 4xx/5xx is `Err(StatusCode)`,
  not an `Ok` response with a status. Turn it off on the config builder if the
  error body matters.
- **`json` is not a default feature in 3.4.2.** Defaults are `rustls` and
  `gzip`. Enabling `json` pulls `serde`/`serde_json` into the graph.
- **`rustls` uses webpki-roots, not the OS store.** A corporate or locally
  trusted CA needs `platform-verifier` (or a configured root set); the default
  will reject it.
- **`read_to_string()` is lossy for `text/*` bodies by default** (invalid UTF-8
  becomes `?`). JSON is `application/json`, so this does not bite for typed
  documents.
- **TLS provider changes are not semver-significant.** Pin the exact version in
  `Cargo.toml` when the dependency graph matters.
