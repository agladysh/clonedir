## Context
Current producer: a worktree-preparation script. It copies ignored dependency and build caches (`node_modules`, `target`) into a new worktree on the same APFS volume, with writers expected to be idle. It needs structure fidelity, verbatim links, independent files, no clobbering, a machine-readable result and no leftover partial state.

## Goals / Non-Goals
- Goals: correct tree reproduction, zero-data-growth cloning on APFS, explicit fallback, atomic publication, detection of concurrent source mutation, owned cleanup including after SIGKILL, bounded space, and accounting that separates `du` from reclaimable space.
- Non-Goals: deduplicating independently written data (for example separately compressed Git objects), retention policy for published copies, point-in-time snapshots of a live tree, Windows/ReFS, and a general sync or copy tool.

## Decisions
- Per-entry walk with per-file `clonefile(CLONE_NOFOLLOW|CLONE_NOOWNERCOPY)` rather than whole-directory `clonefile`. Apple's man page discourages cloning hierarchies with it, and the walk is what allows per-entry refusal, hard-link accounting, verification and a per-file space check.
- Publication by `renamex_np(RENAME_EXCL)` / `renameat2(RENAME_NOREPLACE)`, because plain `rename(2)` silently replaces an empty directory.
- Consistency by re-`lstat` of every recorded source entry (inode, size, mode, mtime, ctime) after the walk. Reading and cloning do not change these, so a mismatch means a writer. The check is conservative: a write between recording and cloning is also reported. Writes through shared memory maps that have not updated mtime are not detected.
- A stage holds only clones or copies of the source, so removing it never destroys unique work. Ownership is established by name prefix, uid and an owner record with a dead pid. A reused live pid keeps the stage (conservative).
- A stage is swept only in the parent being cloned into. That is the scope the tool owns, so there is no global scanner.
- Measurement uses `getattrlist` `ATTR_CMNEXT_PRIVATESIZE`/`CLONEID`/`CLONE_REFCNT`. Free-space deltas are only reported, because other writers share the volume.

## Risks / Trade-offs
- The per-file walk is slower than one directory clone on huge trees. The cost is metadata-bound, and the adopter's trees fit it.
- Removing `--force` breaks scripts that relied on merging into existing directories. That behaviour was unsafe, so it is not kept.
- Linux FICLONE and `renameat2` paths are implemented but were not exercised on Linux in this change.

## Migration Plan
Install with `cargo install --path .`. Callers invoke `clonedir --json SOURCE DEST` per cache and check the receipt.

## Open Questions
- Should a caller retry on exit 3 (source changed) or report it? Reporting it keeps "stop the writers" a caller decision.
