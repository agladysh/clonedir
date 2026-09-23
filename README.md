# clonedir

Clone a directory tree using copy-on-write, correctly.

`clonedir SOURCE DESTINATION` creates `DESTINATION` as an independent copy of `SOURCE`. On APFS
(and Linux filesystems with `FICLONE`) every regular file shares its extents with the source, so
the copy costs metadata but no data blocks until one side is written.

## Semantics

- **Faithful.** Directories and files keep their relative paths, modes and modification times.
  Symlinks keep their targets verbatim and are never followed; their own times are not kept, and
  an absolute link into the source still points into the source. Hard-linked files become
  independent files. On macOS a cloned file also keeps its extended attributes, ACL and flags;
  byte copies, directories and the Linux path keep mode and times only. FIFOs, sockets, devices
  and nested mount points (on Linux also btrfs subvolumes) are refused.
- **No silent copies.** When cloning is impossible (another volume, a filesystem without clones)
  the command fails with exit status 4. `--allow-copy` permits byte copying, and the receipt counts
  the bytes.
- **Never overwrites.** The destination must not exist. The copy is built in a private stage beside
  it and published by an exclusive rename, so the destination appears only complete, and only one
  of several racing clones can win.
- **Consistent or refused.** If any source entry's inode, size, mode, mtime or ctime changed while
  the tree was being cloned, the copy is discarded (exit 3); stop its writers and retry. A copy
  that passes is the tree as it stood at one instant, as far as that metadata shows: writes
  through shared memory maps, and same-size rewrites within one timestamp tick on filesystems
  with coarse timestamps, can go unseen. It is not an application-consistent snapshot: a writer
  paused midway through a multi-file update for the whole run yields a successful copy of that
  intermediate state.
- **Cleans up after itself.** Failures, SIGINT and SIGTERM remove the stage. A stage left by a
  killed run records its pid. `clonedir --sweep PARENT`, or the next clone into that parent,
  removes it once that process is gone. A stage kept with `--keep-failed` is never swept; remove it
  yourself. Stages carry a `.gitignore`, so a stage inside a worktree stays out of `git status`.
  A swept stage held only copies of the source, but once the source has changed or been deleted
  they may be the last copies of that earlier content.
- **Checked, not reserved, space.** It refuses to start below `--min-free` available space (default
  256 MiB), rechecks during the run, and before each byte copy requires room for that file above
  the floor. Concurrent writers can still take the space between a check and a write.

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
