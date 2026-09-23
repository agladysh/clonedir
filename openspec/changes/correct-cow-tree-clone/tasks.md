## 1. Library
- [x] 1.1 Per-entry clone walk with verbatim symlinks, modes, times, hard-link splitting and special-file/mount refusal
- [x] 1.2 Owned stage, exclusive-rename publication, cleanup on failure and signal, stale-stage sweep
- [x] 1.3 Explicit copy fallback with byte accounting and a free-space floor
- [x] 1.4 Post-walk source verification
- [x] 1.5 `measure` / `sharing` accounting

## 2. CLI
- [x] 2.1 Replace clap with dependency-free parsing; remove `--force` and the prompt
- [x] 2.2 `--json`, `--allow-copy`, `--min-free`, `--keep-failed`, `--sweep`, `--measure`; distinct exit statuses
- [x] 2.3 Dry run without probe files

## 3. Tests
- [x] 3.1 Structure, links, modes, times; isolation both ways after mutation; hard links
- [x] 3.2 Refusals leave nothing; failure with read-only partial stage; cancellation; SIGTERM; SIGKILL then sweep
- [x] 3.3 Concurrent mutation detected; fan-out clones; racing threads and processes publish one tree
- [x] 3.4 APFS sharing, divergence, retention after source deletion, free-space observation

## 4. Code Quality
- [x] 4.1 rustfmt and clippy clean
- [x] 4.2 `cargo test` passes (38 tests, macOS APFS)

## 5. Validate and Archive
- [ ] 5.1 `openspec validate correct-cow-tree-clone --strict` (openspec CLI not installed where this was prepared)
- [ ] 5.2 Archive after installation and adopter update
