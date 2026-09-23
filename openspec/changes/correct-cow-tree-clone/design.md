## Context
Current producer: a worktree-preparation script. It copies ignored dependency and build caches (`node_modules`, `target`) into a new worktree on the same APFS volume, with writers expected to be idle. It needs structure fidelity, verbatim links, independent files, no clobbering, a machine-readable result and no leftover partial state.

## Goals / Non-Goals
- Goals: correct tree reproduction, zero-data-growth cloning on APFS, explicit fallback, atomic publication, detection of concurrent source mutation, owned cleanup including after SIGKILL, bounded space, and accounting that separates `du` from reclaimable space.
- Non-Goals: deduplicating independently written data (for example separately compressed Git objects), retention policy for published copies, application-consistent snapshots of a live tree, space reservation, Windows/ReFS, and a general sync or copy tool.

## Decisions
- Per-entry walk with per-file `clonefile(CLONE_NOFOLLOW|CLONE_NOOWNERCOPY)` rather than whole-directory `clonefile`. Apple's man page discourages cloning hierarchies with it, and the walk is what allows per-entry refusal, hard-link accounting, verification and a per-file space check.
- Publication by `renamex_np(RENAME_EXCL)` / `renameat2(RENAME_NOREPLACE)`, because plain `rename(2)` silently replaces an empty directory.
- Consistency by re-`lstat` of every recorded source entry (inode, size, mode, mtime, ctime) after the walk. Reading and cloning do not change these, so a mismatch means a writer. The check is conservative: a write between recording and cloning is also reported. A passing check means every entry was unchanged from before its copy until after the last copy, so the result is the tree at one instant as far as this metadata shows. Writes through already-dirty shared memory maps, and same-size rewrites within one timestamp tick on coarse-timestamp filesystems, are not detected. It is not application consistency: a writer paused between two related writes for the whole walk passes.
- Nobody but clonedir writes into a stage and nothing was published from it, so a dead run's stage is clonedir's own disposable output. It can still hold the only remaining copy of source content that changed or was deleted after the run; that is accepted, and `--keep-failed` exists for anyone who wants a failed stage. A kept stage carries a `kept` marker and is never swept. Ownership is established by name prefix, uid and an owner record with a dead pid. A reused live pid keeps the stage (conservative). Pids are host-local, so parents shared across hosts or pid namespaces are out of scope.
- The sweep is housekeeping: it tolerates concurrent sweeps of the same stage, skips a stage it cannot remove, and never fails the clone that runs it.
- Each stage holds `.gitignore` (`*`). Stages sit beside the destination, often inside a worktree, where their paths match none of the destination's ignore rules.
- A read-only source root is staged with owner access, because moving a directory to a new parent needs write permission on it, and gets its own mode after publication.
- "Destination inside source" compares device and inode of the destination's ancestors with the source as well as path prefixes, because APFS firmlinks alias paths.
- A stage is swept only in the parent being cloned into. That is the scope the tool owns, so there is no global scanner.
- Measurement uses `getattrlist` `ATTR_CMNEXT_PRIVATESIZE`/`CLONEID`/`CLONE_REFCNT`. The space floor is a check, not a reservation, and free-space deltas are only reported, because other writers share the volume.

## Risks / Trade-offs
- The per-file walk is slower than one directory clone on huge trees. The cost is metadata-bound, and the adopter's trees fit it.
- Removing `--force` breaks scripts that relied on merging into existing directories. That behaviour was unsafe, so it is not kept.
- Linux FICLONE and `renameat2` paths are implemented but were not exercised on Linux in this change.

## Migration Plan
Install with `cargo install --path .`. Callers invoke `clonedir --json SOURCE DEST` per cache and check the receipt.

## Open Questions
- Should a caller retry on exit 3 (source changed) or report it? Reporting it keeps "stop the writers" a caller decision.
