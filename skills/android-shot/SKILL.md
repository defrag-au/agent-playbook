---
name: android-shot
description: Use when putting a macroquad app on an Android device — build or install its APK, launch it cold, read SAPP and crash logs, and capture the screen — for an experiment on a real phone or TV. Runs android-shot, which composes shared-crates' mq-apk and owns adb, and reads its report.
argument-hint: <build flags, e.g. "--package au.defrag.montager --library montage_mq --crate montage-mq --abi arm64-v8a">
---

# android-shot

The device half of the macroquad Android loop. `android-shot` builds an APK through
shared-crates' `mq-apk` (or takes an `--apk`), installs it with `adb install -r`, wakes the
device, starts the activity cold, waits, reads the logs, and optionally captures the
framebuffer. The report says whether the app actually came up.

Packaging is **not** this tool. `mq-apk` produces an APK and never needs a phone; this owns the
process that talks to one, the way `browser-shot` owns a browser and `tui-shot` owns a PTY. A
missing packaging feature belongs in `shared-crates/ui/macroquad-apk`, not in a repo's script.

## Run it from the repo that owns the artefacts

`adb` and the SDK come only from defrag-nix's `android` shell, so enter it first:

```sh
cd ~/code/defrag/montager
nix develop .#android
```

Then build and run the app, with the art the app has already drawn:

```sh
cargo run -q -p montage-mq --example launcher_art -- \
    .tmp/android/ic_launcher.png .tmp/android/banner.png

android-shot \
    --package au.defrag.montager --library montage_mq --crate montage-mq \
    --icon .tmp/android/ic_launcher.png --banner .tmp/android/banner.png \
    --abi arm64-v8a \
    --shot .tmp/android/phone.png --expect "cycle 0"
```

- Run from the app's repo root: `.tmp/` paths and the packager's `cargo run` resolve there.
- The APK lands at `.tmp/android/<package>.apk` unless `--out` says otherwise.
- `--library` is the **cdylib** name — `montage_mq`, so the APK contains `libmontage_mq.so`.
  `--crate` is the cargo package, `montage-mq`. They are not the same string.
- `--shot` writes `.tmp/android/phone.png` **and** `.tmp/android/phone.txt`. Without `--shot`
  the report is printed instead.
- `--expect` is checked against the logs; without it a run that draws nothing still passes if
  `SAPP` appeared and the crash buffer is empty.

`android-shot` is packaged by agent-playbook. Once defrag-nix re-locks it, it is on `PATH` in
every defrag devshell, including `android`. Until then, build it and run the copy by path —
inside the android shell, so `adb` is still the shell's:

```sh
nix build ~/code/defrag/agent-playbook#android-shot -o .tmp/android-shot
nix develop .#android -c .tmp/android-shot/bin/android-shot <flags>
```

## Pick the shape

| Job | Command |
| --- | --- |
| Build from source and run | `--package … --library … --crate …` |
| Install an APK that already exists | `--package … --apk <file.apk>` (no build flags) |
| See what would run | `--dry-run` — the packager command and every adb step, no phone, no SDK |

`--dry-run` is the interface: it prints the exact `mq-apk` invocation and the install, wake,
launch, log and screenshot commands without running any of them. Use it when a run fails before
the app starts, to see which value went where.

## Packaging, when it is the question

`mq-apk` resolves miniquad's Java from the `Cargo.lock` and prints its whole plan with
`--dry-run`, needing no SDK:

```sh
mq-apk --package au.defrag.montager --library montage_mq --crate montage-mq \
       --icon i.png --banner b.png --abi arm64-v8a --dry-run
```

- **`--abi arm64-v8a` halves the build** and is what an experiment on a phone wants. No `--abi`
  builds both arm64 and 32-bit ARM, which is for a TV box with a 32-bit userspace (the Google TV
  Streamer).
- **Art is PNGs the app already produced.** `mq-apk` copies them; it does not draw them, and it
  will not invent a pair. `--icon` and `--banner` go together, or `--no-art` for a manifest that
  references no resources.
- The manifest must name the same package `--package` gives; the tool checks rather than
  rewriting it.
- Signing is the standard debug key at `~/.android/debug.keystore`, created if missing, so a
  reinstall updates in place and keeps app data.

## Read the report

The image is the review; `<shot>.txt` is the second check. It leads with what a glance at a
phone misses:

| Field | What it catches |
| --- | --- |
| `launch` | `Status: ok` and `LaunchState: COLD` — a launch that never started |
| `foreground` | whether the activity is the top resumed one — a locked phone says `no` |
| `logs` | `SAPP` lines (miniquad) and `AndroidRuntime` (Java); empty `SAPP` means nothing drew |
| `crash` | must be empty; anything here belongs to this launch |
| `problems` | `none`, or the list that made the exit code 2 |

`foreground yes` is also what makes the PNG the app's screen: when the report says `no`, the
image is whatever happened to be on top of it.

Exit `0` is clean, `2` means it ran but the report has problems, `1` is operational. **Exit 2 is
not a pass**, and the screenshot beside it is still worth looking at.

## Traps

- **A locked phone swallows the launch.** `am start` succeeds, the app logs, and the capture is
  a clock. The tool wakes the screen and dismisses a non-secure keyguard; a PIN or pattern stays,
  and `foreground no` is the report saying so. Unlock the phone; do not try to bypass it.
- **A charging phone with a screensaver runs a *dream*, which is not a keyguard.**
  `wm dismiss-keyguard` does not clear it, and miniquad panics in `eglMakeCurrent` behind one.
  The tool taps the centre when `mWakefulness=Dreaming`; if you change screensaver or stay-awake
  settings to diagnose, put them back.
- **`am start -W -S` is a cold start on purpose.** Without `-S`, `am start` hands the intent to
  the instance already in the task and the run tests the previous build.
- **`SAPP` is miniquad's logcat tag**, and the buffer is cleared before install so a crash in
  the report belongs to this run. A Java crash shows under `AndroidRuntime`; both are read.
- **Min API is 30.** A device older than Android 11 refuses the install; that is a device
  mismatch, not a signing problem.
- **A different signing key cannot update an installed app** — `INSTALL_FAILED_UPDATE_INCOMPATIBLE`
  means uninstall, which loses app data. Say that before suggesting it, and use the debug
  keystore the tool already chooses.
- **An unauthorized or ambiguous device is named, not guessed.** Accept the USB debugging prompt,
  or pass `--device <serial>` when more than one is attached.
- **`--at 3` is three seconds from launch to the log read.** A hosted app that fetches before its
  first frame may need more, and `--expect` is what makes the wait mean something.

## Do not

- hand-run `adb install`, `am start`, `logcat` or `screencap` — the tool owns the device loop and
  its report
- kill or restart the adb server, or kill adb by name
- uninstall the app, or install with `-d`, to make an update succeed
- change device settings to force a run through
- claim the app ran without reading the PNG and the report
- add Android packaging to the app's repo — change `mq-apk` instead

Tool reference: `~/code/defrag/agent-playbook/tools/android-shot/` and the Android section of
the repository README.
