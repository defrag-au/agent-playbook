# at-continue-thread

A bounded, text-only reading of one Zed agent thread, enough to continue it in a new one.

```sh
at-continue-thread find exoplanet
at-continue-thread digest "Exoplanet Set Rendering Progress Review" --budget-kb 64
```

## Why it exists

A Zed thread stores every tool result, including each screenshot the agent read, as base64. Those
bytes are re-sent on every turn. A visual pass reaches tens of megabytes long before it reaches the
context window, and a provider can refuse the request outright:

```
413 Payload Too Large {"error":{"message":"Failed to buffer the request body: length limit exceeded"}}
```

Zed's automatic compaction is token-based, so it never fires for a thread that is half empty by
tokens and far too large by bytes. Compaction cannot rescue such a thread either, because
compaction is itself a request over the same body.

This tool reads the thread instead. It keeps the user's direction, the source files touched, and a
head-and-tail window of the agent's activity; tool results are clipped to a line, images become a
placeholder naming the file and its size, and private thinking is dropped. A new thread reads the
digest and carries on. The old thread is untouched.

## Verbs

| Verb | Answers |
| --- | --- |
| `find <fragment>` | Which threads match this title fragment: id, last updated, stored size, title |
| `digest <id-or-title>` | Enough of one thread to continue it: user messages, files, activity |

`digest` resolves by id, or by a unique fragment of the title. An ambiguous fragment is refused
(exit 2) and the candidates are listed; nothing is guessed.

## What the digest carries

- Every user message, in order, with its old message index (`[m69]`).
- Every source path read or written, with the tools and counts. Renders are excluded — they are
  named where they were read, in the activity placeholder.
- The head and the tail of the agent's activity, to a byte budget (`--budget-kb`, default 64,
  ceiling 256). The middle is what goes, and the `#` bounds say how much.
- Tool calls with their path, command or query and a clipped result; errors are marked `ERROR:`.
- Compaction summaries, if the thread had any.

Images become `[image omitted, 1189 kB]` with the path from the matching call. Thinking is counted
and omitted. The digest never contains base64.

## Safety

The store is opened with SQLite's read-only flag and the `query_only` pragma. There is no write
path in either verb, no subprocess, and no network. `find` reads no blobs.

## Tests

```sh
direnv exec . cargo test -p zed-thread-tools digest
direnv exec . cargo clippy -p at-continue-thread -p zed-thread-tools --all-targets -- -D warnings
```
