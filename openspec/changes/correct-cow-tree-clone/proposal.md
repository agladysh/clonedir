## Why
`clonedir` 0.1 wraps `clonedir_lib` 0.1.1, whose recursion passes the destination root instead of `destination/<name>` for each subdirectory. The installed binary therefore flattens every tree deeper than one level into the destination root. Same-named files at different depths silently overwrite one another through the `reflink_or_copy` byte-copy fallback, and empty directories vanish. It still exits 0. Reproduced 2026-09-23 on `~/.cargo/bin/clonedir`: `a/b/same` and `c/same` produced one `same` containing `C`. The library also follows symlinks (file links are dereferenced; directory and dangling links are dropped) and silently byte-copies whatever it cannot clone. It publishes non-atomically into an existing destination, leaves partial trees on failure, and writes a fixed `.clonedir_temp_check` probe into the destination parent.

A known adopter, a worktree-preparation script that copies ignored dependency and build caches, works around the symlink behaviour. It still delegates link-free subtrees such as a Rust `target/` to this binary, and those are exactly the trees the flattening corrupts. Its tests used trees one level deep, so they could not see the defect.

## What Changes
- **BREAKING** Replace the `clonedir_lib` and `clap` dependencies with an in-crate library (`clonedir::clone_tree`) that has no dependencies. It walks the tree without following links, clones each regular file (APFS `clonefile`, Linux `FICLONE`) and recreates symlinks verbatim. It keeps modes and times, refuses special files and nested mounts, and splits hard links into independent files.
- **BREAKING** Never overwrite or merge. The destination must not exist. `--force` and the confirmation prompt are removed. The tree is built in an owned stage beside the destination and published by an exclusive rename.
- **BREAKING** No silent byte copies. Cross-filesystem and unclonable cases fail (exit 4) unless `--allow-copy` is given, and the receipt counts every copied byte.
- Source entries are re-examined after cloning. A change during the clone discards the copy (exit 3).
- Stages are removed on failure, SIGINT and SIGTERM. A killed run's stage records its pid and is swept by `--sweep` or by the next clone into the same parent once that pid is gone.
- `--min-free` bounds space use (default 256 MiB).
- `--json` prints a machine receipt or error. `--measure` reports logical, `du`-allocated and APFS private bytes separately.
- The dry run no longer writes a probe file.

## Impact
- Affected specs: cli
- Affected code: `Cargo.toml`, `src/lib.rs`, `src/sys.rs`, `src/main.rs`, `tests/`
- Adopters: a caller that copies caches can delegate whole trees, links included, and drop any link-walking workaround. Scripts passing `-f` must choose a fresh destination instead.
