---
name: continue-thread
description: Use when the user says "Continue <thread name>", "resume <thread>", or "pick up where <thread> left off", or when a Zed agent thread can no longer be sent — a 413 Payload Too Large, "Failed to buffer the request body", a thread too large to compact, or a turn that fails before the model answers. Reads a bounded, text-only digest of the named thread with at-continue-thread and continues the work in this new thread.
---

# Continuing a thread that can no longer be sent

## What brought you here

- The user typed `Continue <thread name>` in a **new** thread.
- A thread failed with `413 Payload Too Large` or `Failed to buffer the request body: length limit
  exceeded`.
- `/compact` failed, or another turn on the thread is impossible.

Do not try to repair the old thread from inside it. Every retry — compaction included — is a
request over the same body, so it fails the same way. Read it, and continue here.

Why it happens: the thread stores every screenshot the agent read as base64, and every turn re-sends
them. A visual pass reaches tens of megabytes while the token count is still half the model's
window, so Zed's token-based auto-compaction never fires. The failure is bytes, not context.

## The procedure

**1. Resolve and digest.**

```sh
mkdir -p .tmp
at-continue-thread digest "<thread name>" --budget-kb 64 > .tmp/continue-thread.md
```

| Exit | Meaning | What to do |
| --- | --- | --- |
| `0` | One thread matched; the digest is in the file | Continue |
| `1` | Nothing matched | Run `at-continue-thread find "<shorter fragment>"`, then ask |
| `2` | Several matched; the output lists `id · title` | Ask which, or pass the id. Never guess between candidates |

**2. Read the file** with `read_file` (`.tmp/continue-thread.md`).

**3. Say where the work stood, in two or three lines**: the last user request, the last edit or
command, and the last result that decided the next step. That is your working state; do not echo
the digest back.

**4. Continue.** Re-read the source files the digest names — they may have moved since — run the
command that was next, and make the change. The `[mNN]` markers are the old thread's message
indices and are citable.

**5. If the digest is too thin, widen it** rather than guessing:

```sh
at-continue-thread digest "<id>" --budget-kb 128 > .tmp/continue-thread.md
```

The tail holds the most recent activity. If you need one specific fact, ask for that and read the
relevant source file or re-run the command instead of widening the whole digest.

## What is in the digest, and what went

- **Kept:** every user message, every source path touched (renders excluded), the head and tail of
  the agent's activity, tool calls with their argument and a clipped result, compaction summaries.
- **Dropped:** image payloads — replaced by `[image omitted, N kB]` with the file path — private
  thinking, and the middle of long activity logs. The trailing `#` bounds state exactly how much.
- Images are dropped **for size**, not because they are uninteresting. If a current render must be
  seen, take a fresh capture and read that one image once; do not try to recover the old ones.

## If the tool is not on PATH

The playbook's built copy works without any shell setup:

```sh
~/code/defrag/agent-playbook/target/debug/at-continue-thread digest "<thread name>" --budget-kb 64
```

If it is missing, build it once:

```sh
direnv exec ~/code/defrag/agent-playbook cargo build --manifest-path ~/code/defrag/agent-playbook/Cargo.toml -p at-continue-thread
```

It is read-only: it opens Zed's `threads.db` with SQLite's read-only flag, writes nothing, and
spawns nothing. The old thread is not modified, archived or compacted.

## The in-place path is a different tool

If the user needs the **same thread object** — not a new thread — `zed-thread-recover` from
`tools/zed-thread-tools` appends a `Compaction(Summary)` message to the stored thread. It requires
quitting Zed and writes its own backup. That is not this flow: "Continue \<name\>" deliberately
starts a clean thread, which is what makes the next request small.
