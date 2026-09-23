# clonedir

Clone a directory tree using copy-on-write, correctly.

`clonedir SOURCE DESTINATION` creates `DESTINATION` as an independent copy of `SOURCE`. On APFS
(and Linux filesystems with `FICLONE`) every regular file shares its extents with the source, so
the copy costs metadata but no data blocks until one side is written.

## Semantics

- **Faithful.** Directories, files and symlinks keep their relative paths, modes and modification
  times. Symlinks are recreated verbatim and never followed. Hard-linked files become independent
  files. FIFOs, sockets, devices and nested mount points are refused.
- **No silent copies.** When cloning is impossible (another volume, a filesystem without clones)
  the command fails with exit status 4. `--allow-copy` permits byte copying, and the receipt counts
  the bytes.
- **Never overwrites.** The destination must not exist. The copy is built in a private stage beside
  it and published by an exclusive rename, so the destination appears only complete, and only one
  of several racing clones can win.
- **Consistent or refused.** If the source changes while it is being cloned, the copy is discarded
  (exit 3). Stop its writers and retry.
- **Cleans up after itself.** Failures, SIGINT and SIGTERM remove the stage. A stage left by a
  killed run records its pid. `clonedir --sweep PARENT`, or the next clone into that parent,
  removes it once that process is gone.
- **Bounded.** It refuses to proceed below `--min-free` available space (default 256 MiB).

## What copy-on-write does not do

Only the files `clonedir` clones share extents. Data written independently (a rebuilt artifact, a
freshly compressed Git object, a second download) is not deduplicated. Deleting the source does not
free space while a clone still holds its blocks; the clone simply becomes their sole owner. `du`
counts shared extents once per file, so it overstates what deleting a clone would free.
`clonedir --measure PATH` reports logical, `du`-allocated and APFS private (unshared) bytes
separately.

## Usage

```
clonedir [OPTIONS] SOURCE DESTINATION
clonedir --sweep [--json] PARENT
clonedir --measure [--json] PATH...

  -v, --verbose        Report what was done
  -n, --dry-run        Check the inputs and report the plan without creating anything
  -q, --quiet          Suppress non-error output (overrides --verbose)
      --json           Print a JSON receipt (or error) on stdout
      --allow-copy     Copy bytes where cloning is impossible
      --min-free SIZE  Refuse to proceed below SIZE available (default 256M; K/M/G suffixes)
      --keep-failed    Keep a failed stage for inspection instead of removing it
```

Exit status: 0 success, 1 error, 2 usage, 3 source changed, 4 cannot clone without
`--allow-copy`, 5 insufficient space, 6 destination exists, 130 interrupted.

A successful `--json` run prints one object:

```json
{"ok":true,"source":"/abs/src","destination":"/abs/dst","filesystem":"apfs","directories":6,
 "regular_files":40,"symlinks":1,"cloned_files":40,"copied_files":0,"logical_bytes":307,
 "copied_bytes":0,"hardlinked_files":0,"swept_stages":[],"available_before":…,"available_after":…}
```

A failure prints `{"ok":false,"error":{"kind":"SourceChanged","path":…,"message":…,"kept_stage":null}}`.

## Library

```rust
let receipt = clonedir::clone_tree(src, dst, &clonedir::Options::default())?;
let usage = clonedir::measure(dst)?; // logical, allocated, private bytes
```

`Options` carries the fallback, the space floor, a cancellation flag, a per-entry hook and whether
to keep a failed stage. The crate has no dependencies.

## Building and testing

```bash
cargo build --release
cargo test        # small fixtures; the physical-allocation tests run on APFS
cargo install --path .
```

## License

MIT; see LICENSE.
