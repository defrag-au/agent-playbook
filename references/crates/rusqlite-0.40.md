# rusqlite 0.40.2 (rusqlite/rusqlite)

Bundled SQLite is 3.53.2 via `libsqlite3-sys` 0.38.2.

## At a glance

- `bundled` compiles SQLite from source; right for a standalone tool, wrong for a library that does not own its database.
- Read-only is `OpenFlags::SQLITE_OPEN_READ_ONLY`; `pragma query_only` is a second guard, not the guarantee.
- Opening a missing file read-only fails instead of creating an empty database.
- This sheet answers read-only opens, typed rows, statements and transactions.

## Cargo.toml

```toml
rusqlite = { version = "0.40", features = ["bundled"] }
```

`bundled` compiles SQLite from source with the `cc` crate. It avoids depending on the host's
SQLite and is the right choice for a standalone tool. Do not enable it in a library that does
not control its own database.

## Open a database read-only

```rust
use rusqlite::{Connection, OpenFlags};

let connection = Connection::open_with_flags(
    path,
    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
)?;
connection.pragma_update(None, "query_only", true)?;
```

`SQLITE_OPEN_READ_ONLY` is the real guarantee; `query_only` is a second guard on the connection.
Opening a missing file in read-only mode fails rather than creating an empty database.

## Read a row into a typed struct

```rust
use rusqlite::Connection;

struct Header {
    id: String,
    updated_at: String,
    compressed_bytes: u64,
}

let mut statement = connection.prepare(
    "SELECT id, updated_at, length(data) FROM threads ORDER BY updated_at DESC",
)?;
let rows = statement.query_map([], |row| {
    Ok(Header {
        id: row.get(0)?,
        updated_at: row.get(1)?,
        compressed_bytes: row.get::<_, i64>(2)?.max(0) as u64,
    })
})?;

let headers = rows.collect::<Result<Vec<_>, _>>()?;
```

`length(data)` is an `i64`, not a `u64`.

## Iterating a `query_map` result — the trap

`query_map` returns `MappedRows`, which is an iterator of `Result<Row, Error>`. `.next()` is
`Option<Result<...>>`, not `Result<Option<...>>`, so `rows.next()?` does not compile:

```rust
// no
let row = rows.next()?.ok_or(Error::Missing)?;

// yes
let row = rows
    .next()
    .ok_or(Error::Missing)?
    .map_err(Error::from)?;
```

## Read a blob

```rust
let data: Vec<u8> = row.get(4)?;
```

Reading the same column twice as `Vec<u8>` copies the blob twice. Read it once and compute the
length from the `Vec`.

## Write inside a transaction

```rust
let mut connection = Connection::open(path)?;
connection.busy_timeout(std::time::Duration::from_secs(5))?;

let transaction = connection.transaction()?;
transaction.execute(
    "UPDATE threads SET summary = ?1, data = ?2 WHERE id = ?3",
    rusqlite::params![summary, data, id],
)?;
transaction.commit()?;
```

`params!` borrows its arguments, so a `String` used in the macro is still available afterwards.

## Consistent backup

```rust
let escaped = path.replace('\'', "''");
connection.execute_batch(&format!("VACUUM INTO '{escaped}'"))?;
```

`VACUUM INTO` writes a consistent copy without stopping the connection. Escape single quotes in
the path; it is a SQL string literal.

## Gotchas

- `bundled` makes the first build slow and requires a C compiler. The resulting binary does not
  need the host SQLite.
- A read-only connection still needs the directory to be readable.
- `Connection::open` creates a missing database; check `Path::exists` first if that is not wanted.
- `row.get::<_, Option<String>>(index)` for nullable columns; a bare `String` errors on `NULL`.
