# zstd 0.14.3 (gyscos/zstd-rs)

## At a glance

- Whole-buffer work is `zstd::stream::decode_all` / `encode_all`; `&[u8]` is already a `Read`.
- Level 3 is what Zed uses for thread blobs; the level is not part of the format, so any level decodes.
- Errors are `std::io::Error`, not a zstd error type; a plain-text input fails as `Invalid input`.
- `decode_all` allocates the whole output; use `Decoder` + `io::copy` for a very large frame.
- This sheet answers frame decompression/compression and the streaming API.

## Cargo.toml

```toml
zstd = "0.14"
```

## Decompress a frame

```rust
let json = zstd::stream::decode_all(data)?;
```

`decode_all<R: Read>(source: R) -> std::io::Result<Vec<u8>>`. `&[u8]` implements `Read`, so a
compressed blob can be passed directly. The function sizes the `Vec` from the frame's recorded
decompressed size when that is available, which is much faster than growing it.

## Compress

```rust
let compressed = zstd::stream::encode_all(json.as_slice(), 3)?;
```

Level 3 is what Zed uses for thread blobs. The level is not part of the format; any level decodes
with the same function.

## Gotchas

- Errors are `std::io::Error`, not a `zstd`-specific type.
- The input must be a zstd frame. A truncated or plain-text blob fails with an I/O error such as
  `Invalid input`.
- `decode_all` is convenient but allocates the whole output. For a very large frame, use
  `zstd::stream::Decoder` and `std::io::copy` into a buffer you already own.
- `zstd::zstd_safe::get_frame_content_size(data)` (or `decompress_size` on a decoder) tells you
  the recorded size before allocating.
