# Browser automation: follow-up requirements

**Status:** follow-up to [`browser-automation.md`](browser-automation.md), 2026-10-24.
`tools/browser-shot` was built against the original contract; these are the gaps found by using it.
**R1, R2 and R3 are implemented and were verified on this machine on 2026-10-24** — see
*Verification* below. `eval … out=` is documented in `tools/browser-shot/SCENARIO.md`, and the
timestamp and rate fields are in the sidecar reference in `tools/browser-shot/README.md`. R3 was
also verified against the real Island Funk bundle — 56.2 Hz headless, see *Corrected* below. What
still blocks the two-peer spike is client-side, not tool-side.

## Why there is a second document

The original asked for evidence in two shapes: an image, and a pass/fail. Both work, and the
warnings channel makes them trustworthy. The P2P overlay spike
(`augminted-bots/docs/P2P_OVERLAY_DESIGN.md` §12) needs a third shape — **a series over time** —
and that is the one the tool does not produce. It can tell me whether a check passed. It cannot
tell me *how much fresher* the overlay was, which is the only question the spike exists to answer.

## What was verified, and what was not

Run against a purpose-built probe page on this machine, one peer, 390x844, DPR 3, touch:

```text
result   pass
image    900x600 (element canvas)          # a 300x200 canvas at DPR 3
frame rate  59.5 Hz (63 -> 123 frames in 1009 ms)
input    KeyW down @ frame 228 … KeyW up @ frame 240 (held 12 frames)
check pass  window.__keys[0].trusted === true
check pass  window.__keys[1].frame - window.__keys[0].frame === 12
```

Re-verified here: requirement 3 (element capture at real pixel dimensions), 8 (frame-relative
hold), 9 (sidecar records input and focus), 12 (warnings channel). Not re-verified by me and taken
from the tool's own README: the two-peer DataChannel case, network emulation, the loss hook, and
SIGINT cleanup.

The warnings channel earned its place in that run. It caught two faults in the probe page that I
had not noticed:

```text
warnings  KeyW down went to body; focus a canvas before sending keys;
          canvas backing store 300x200 is smaller than 900x600; the captured pixels are an upscale;
          renderer is software: … SwiftShader …
```

Every key had gone to `document.body`, because a canvas without `tabindex` cannot take focus. The
run still reported `pass`, which is correct — the run was valid and the page was wrong. Without
the warning I would have asserted "keyboard input works" from a test in which no key reached the
game.

### R1 and R2, verified

```text
output   .tmp/series-run/samples.json          # 51,918 bytes
         len 600
         first {"t":1792827721103.221,"overlay_lag":12.3456,"confirmed_lag":150.5,"buffered":3}
         last  {"t":1792827751053.221,"overlay_lag":12.790044,"confirmed_lag":150.5,"buffered":0}

captured 2026-10-24T07:48:40.952Z
ready    … · t=1792828115979.481; window.__ready === true = true · t=1792828117496.666
input    KeyW down @ frame 149 · t=1792828117699.526 · focus BODY · scroll none
         KeyW up @ frame 161 (held 12 frames) · t=1792828117902.209
 eval      window.__btSamples · t=1792828118207.527 out=samples.json

browser-shot run: evaluate on p: cannot encode a cyclic object at $.self     # exit 1
```

R1: 600 samples survive with fractional precision intact, no cap, and a value that cannot be
encoded fails the step by type and path rather than being stringified. R2: every event line carries
fractional epoch milliseconds and `captured` is ISO with milliseconds.

### An observed constraint on input timing

The same run asked for `hold=4` frames and measured 12, and the tool said so:

```text
key hold requested 4 frames, measured 12; a CDP round trip is ~6 frames on this transport
```

At ~59 Hz that is a floor of roughly 200 ms on any held input. It does not affect the overlay
measurement, which samples lag inside the page, but it does mean a scenario cannot drive a
frame-precise input *sequence*. Holds are for putting the game in a state; measurements come from
`eval … out=`.

### The warnings channel, again

`KeyW down went to body` fired in this run too, because the probe canvas still had no `tabindex`.
That is the second time a warning named a fault in the page that a passing run would otherwise have
hidden, which is worth noting when judging whether to keep the warnings or fail on them. Keeping
them as warnings was the right call: the run was valid and the page was not.

## R1 — An `eval` result must be persistable

**Blocks:** the two-peer overlay spike. Nothing else.

`eval … capture=name` binds a value for `${name}` interpolation in later steps, and nothing more.
There is no way to put a structured result on disk, so a measurement has to be reduced inside the
page to a single number an `assert` can compare — which throws away the distribution. Lag is p50
versus p99, not a boolean.

### Requirements

- `eval <peer> "<expression>" out=<path>` writes the value, JSON-serialised, under the artifact
  directory. Path rules as `capture`: relative to `--artifacts`.
- **The full value survives.** Arrays, nested objects, and fractional numbers, at the precision the
  page produced. No stringification, no `"[object Object]"`, no truncation at some sample cap.
  A ten-second run at 20 Hz per peer is 600 samples across four fields; that is the normal case,
  not the large one.
- **Written even when the run fails.** A measurement run that trips one assertion still has data,
  and exit `2` is not a reason to discard it.
- **A value that will not serialise is an error naming what could not be encoded**, not a silent
  substitution. A `Map`, a cyclic object or a `BigInt` must fail loudly.
- `run.json` lists the files written, so a reader knows the artefact set without globbing.

### Why not reuse `capture=`

`capture=` is an interpolation binding and its lifetime is the run. Making it also mean "write this
to disk" would give one name two jobs and leave no way to bind a value without creating an
artefact. Keep them separate.

### Acceptance

A scenario writes a 600-element array of `{t, overlay_lag, confirmed_lag, buffered}` and the file
parses back to 600 elements with `overlay_lag` preserved to at least 0.01 ms.

A second scenario evals a cyclic object and exits non-zero naming it.

## R2 — Millisecond timestamps on tool events

**Blocks:** nothing. It removes a join between the sidecar and page logs.

`input` carries frames (`KeyW down @ frame 228`) and `captured` is second-resolution ISO. A page
can timestamp itself well (`performance.timeOrigin + performance.now()`), so the page side has a
good clock and the tool side does not.

### Requirements

- `input`, `capture`, `eval`, `wait` and `burst` lines carry a fractional epoch millisecond value
  (`t=1750.123`) in addition to the frame number where there is one. Wall clock, not run-relative —
  the page's clock is wall clock, and the join should not need arithmetic against a value printed
  once in a header.
- `captured` keeps its ISO form and gains the same fraction, so one line is enough.
- Ordering two peers' events and their page logs on one timeline must be possible to sub-millisecond.

### Why it matters for this project

"Peer A sent at t=1203.4, peer B first rendered it at t=1219.1" is the measurement. `captured` at
second resolution cannot answer whether a PNG was taken before or after the packet it is meant to
show landed.

## What the spike will ask the application for

Not requirements on the tool, but they justify R1's shape and they are the next piece of work, so
they are recorded here rather than rediscovered later.

| Hook | Purpose |
| --- | --- |
| `window.__frames` | the counter `wait … frames=` and `hold=` read; `frame_counter=` can name another |
| `window.__ready` | set after wasm instantiation and a first frame; nothing in the DOM signals it |
| `window.__btOffer()` / `__btAccept(answer)` | DataChannel offer/answer, signalled through the room |
| `window.__btSamples` | the array R1 dumps — `[t, overlay_lag, confirmed_lag, buffered]` |
| channel created **after** `preload` | the loss hook wraps `RTCDataChannel.prototype.send`, so a channel made before it is unwrapped |

The last row is a real constraint: the game must not construct its data channel at module
evaluation time if the hook is to apply. Creating it in response to the first scenario call is the
simple way to guarantee that.

## Deliberately not asked for

- **Process orchestration for a local server.** A scenario that depends on `wrangler dev` failing
  at its first `wait` names the predicate and its last value, which is a good enough diagnosis.
- **A clock-freeze or `--at` equivalent.** `wait frames=N` plus a burst covers review; a
  deterministic seek is a larger feature for a smaller gain.
- **Jitter emulation.** CDP has no knob and rejecting it is right.
- **A pinned browser.** Accepted in the original document; the sidecar names the version, so a
  difference between machines is attributable rather than mysterious.

## Order

R1 first — it is the difference between "the overlay is worth building" and "the overlay passed a
check". R2 whenever it is convenient.

**Both are done** (2026-10-24, verified). The application hooks in *What the spike will ask the
application for* are now in place except the signalling pair — see *Status of the application
hooks* below. **R3 is done** (2026-10-24, verified against probes).

---

## R3 — Report the frame rate on a page that never settles

**Added 2026-10-24, after the application hooks landed.** Blocks knowing whether a capture of a
game is representative at all.

`frame rate` read **`not measured`** in all three captures of the real Island Funk bundle: with
`--no-settle` and without it, at DPR 3 and at DPR 1. The same tool measured `59.5 Hz` on a
synthetic page carrying the same `window.__frames` counter in the same session, and the frame wait
was satisfied (`window.__frames = 27 (waited 20)`), so neither the counter nor the wait is the
problem.

### Requirements

- Report the measured rate whenever a frame counter is available, whether or not the page settled.
  A game canvas never settles; for this project that is the normal case, not the exception.
- Make the rate assertable, so a scenario can state the rate it needs instead of a human reading
  it. `expect "window.__frames > 20"` works but cannot express a rate.
- When a rate genuinely cannot be measured, name the precondition that failed, the way a readiness
  timeout names its predicate. `not measured` is a bound nobody read.

### R3, verified

The rate is now measured when a report is built, not at launch, so a counter that a wasm app
defines late is present by then. `wait <peer> rate=30` makes the rate a condition; a failed wait
names the rate it last saw, and a measured rate under 20 Hz is a warning in the sidecar.

```text
# samples page, single shot
frame rate   60.3 Hz (71 -> 132 frames in 1011 ms)

# rate=100 wait on a 60 Hz page
ready        …; frame.rate = 60.3 Hz (needs 100.0) FAILED · t=…
result       FAIL

# ~10 Hz page, rate=20 wait
frame rate   9.9 Hz (44 -> 54 frames in 1006 ms)
warnings     readiness failed: frame.rate = 10.9 Hz (needs 20.0);
             measured 9.9 Hz: under 20 Hz a capture or interaction is not representative; …

# page with no window.__frames
frame rate   not measured: frame counter "window.__frames" is not a finite number
```

The late-counter case was also checked with the stability loop: a page defining `window.__frames`
after two seconds reported `60.3 Hz`. The `--no-settle` path captures immediately and names the
missing counter instead of reporting zero. The Island Funk bundle was then re-run against the
shipped binary and reported `56.2 Hz`; see *Corrected* below, which also retracts a claim made from
the same data.

## Corrected: headless is fast enough, and the earlier reading was wrong

A previous revision of this section claimed headless rendered this bundle at single-digit fps, and
that the two-peer spike must therefore run `headed`. **That was wrong.** Measured against the real
bundle, served over loopback at 390x844 DPR 3 under SwiftShader:

```text
frame rate   56.2 Hz (105 -> 162 frames in 1014 ms)      # single shot, --no-settle
peer 0  pass  56.5 Hz                                   # wait p rate=30
peer 0  FAIL  56.3 Hz                                   # wait p rate=240, exit 2
```

The mistake was reading `window.__frames = 27` a couple of seconds after `__ready` as a steady-state
rate. It is not one: the counter starts at the first drawn frame, so that window covers wasm
instantiation, font loading and the first asset decode. The 56 Hz figure is a one-second sample
taken once the page has settled — precisely the distinction R3 exists to make, and the reason a
named rate is worth more than a frame count.

What follows:

- **`headed` remains the right mode for judging what a player sees** (`look-at-the-game`'s "a
  capture is not the window"), but it is **not required for frame rate** on this client. The earlier
  claim would have sent someone chasing a non-problem and made the spike look harder than it is.
- **Two peers measured: 51.5 Hz and 50.6 Hz**, at 390x844 DPR 3 in headless, with `wait a,b rate=30`
  passing for both. Two SwiftShader processes on one machine cost roughly one frame in ten against
  the single-peer 56 Hz — well inside what the spike needs. `headed` is a preference for judging
  motion, not a requirement for the measurement.
- **Nothing measured before `__ready` is representative.** A scenario asserting a rate must wait
  for it first, or it is measuring the boot.
- **The client reloads when its world connection fails, and that races the report.** With no worker
  on the origin, peer 1 reported `rate ?` in the summary and `frame counter "window.__frames" is not
  a finite number` in its sidecar — while the wait itself had measured `53.0 Hz`. The page had
  navigated between the wait and the report. Load the client with `?offline=1` for any capture that
  is not deliberately testing a world. The flag needs a non-empty value: `launch_query` treats
  `?offline` and an absent parameter the same, so a bare `?offline` does nothing.

## Status of the application hooks

Implemented in `games/island-funk/apps/island-funk` and verified against a real bundle, served over
loopback and captured at 390x844 DPR 3:

| Hook | State |
| --- | --- |
| `window.__frames` | done — `src/probe.rs`, `web/probe.js`; read `27` after `wait frames=20` |
| `window.__ready` | done — set by the first drawn frame |
| `window.__btSamples` / `__btSample()` | done — bounded at 4096 records, empty until something measures |
| `__btOffer()` / `__btAccept(answer)` | **not done** — needs the JS peer shim (build-order item 4 in `augminted-bots/docs/P2P_OVERLAY_DESIGN.md`) |

There is deliberately no Rust `sample()` import: the overlay's telemetry comes from the JS shim,
which can call `window.__btSample` directly, so a wasm import would have had no caller.
