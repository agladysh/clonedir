## ADDED Requirements

### Requirement: Faithful Tree Reproduction
The CLI SHALL reproduce every directory, regular file and symbolic link of the source at the same relative path in the destination, keeping file and directory modes and modification times. It SHALL recreate symbolic links verbatim without following them. Each hard-linked source file SHALL become an independent destination file. It SHALL refuse FIFOs, sockets, devices and mount points inside the source.

#### Scenario: Nested same-named files
- **WHEN** the source holds `a/b/same` and `c/same`
- **THEN** the destination holds both, at those paths, with their own contents

#### Scenario: Links are kept, not followed
- **WHEN** the source holds relative, absolute, directory and dangling symbolic links
- **THEN** the destination holds symbolic links with identical targets

#### Scenario: Independence after mutation
- **WHEN** a destination file is written, truncated or removed, or the source is written
- **THEN** the other side is unchanged

#### Scenario: Special file
- **WHEN** the source contains a FIFO
- **THEN** the CLI exits 1 naming it, and neither a destination nor a stage remains

### Requirement: Copy-on-Write Without Silent Copies
The CLI SHALL materialize regular files by sharing extents with the source. Where the source and destination cannot share extents, it SHALL fail with exit status 4 unless `--allow-copy` is given. With `--allow-copy` it SHALL copy bytes and report the files and bytes copied.

#### Scenario: Cross-filesystem destination
- **WHEN** the destination parent is on another filesystem and `--allow-copy` is not given
- **THEN** the CLI exits 4 before creating anything

#### Scenario: Cloned receipt
- **WHEN** a clone on APFS succeeds with `--json`
- **THEN** the receipt reports `copied_bytes` 0 and a `cloned_files` count equal to the regular files

### Requirement: Atomic Publication
The CLI SHALL build the copy in a private stage beside the destination and publish it with an exclusive rename. It SHALL never overwrite or merge into an existing destination.

#### Scenario: Read-only source root
- **WHEN** the source directory itself is read-only
- **THEN** the clone is published with the source's mode

#### Scenario: Destination exists
- **WHEN** the destination exists as a file, directory or symbolic link
- **THEN** the CLI exits 6 and the existing entry is unchanged

#### Scenario: Racing clones
- **WHEN** several clones of one source target one new destination at once
- **THEN** exactly one succeeds, the others exit 6, and the published tree is complete

### Requirement: Source Consistency
The CLI SHALL re-examine every copied source entry after copying. If any entry changed, it SHALL discard the copy and exit 3. This establishes a filesystem-level instant as far as inode, size, mode, mtime and ctime show; it does not establish application-level consistency.

#### Scenario: Concurrent writer
- **WHEN** a source file is rewritten, or an entry added, after it was cloned but before the clone completes
- **THEN** the CLI exits 3 and nothing is published

### Requirement: Owned Cleanup
The CLI SHALL remove its stage on failure and on SIGINT or SIGTERM, including stages holding read-only directories. A stage left by a killed process SHALL record the owner pid. `--sweep PARENT` and every later clone into the same parent SHALL remove stages whose owner is no longer running, and only those.

#### Scenario: Terminated run
- **WHEN** a running clone receives SIGTERM
- **THEN** it exits 130 leaving neither a destination nor a stage

#### Scenario: Killed run
- **WHEN** a running clone is killed with SIGKILL
- **THEN** its stage remains with an owner record, and `--sweep` on the parent removes it

#### Scenario: Live owner
- **WHEN** a stage's owner pid is running
- **THEN** sweeping leaves it in place

#### Scenario: Kept stage
- **WHEN** a failed run was given `--keep-failed` and a later clone into the same parent, or `--sweep`, runs
- **THEN** the kept stage remains

#### Scenario: Concurrent sweeps
- **WHEN** several clones into one parent start while it holds a dead run's stage
- **THEN** all of them succeed and the stage is removed

#### Scenario: Unremovable stale stage
- **WHEN** a dead run's stage cannot be removed
- **THEN** a clone into the same parent still succeeds and the stage is left for a later sweep

#### Scenario: Stage inside a Git worktree
- **WHEN** a stage exists inside a Git worktree
- **THEN** `git status` does not list it

### Requirement: Space Bound
The CLI SHALL refuse to start, and stop, when available space on the destination filesystem is below `--min-free` (default 256 MiB), plus the bytes about to be copied when copying. It SHALL exit 5 and remove its stage. The floor is checked at start, every 256 entries and before each byte copy; it is not a reservation, so concurrent writers can still exhaust space between a check and a write, in which case the clone fails and removes its stage.

#### Scenario: Floor above available space
- **WHEN** `--min-free` exceeds available space
- **THEN** the CLI exits 5 and creates nothing

### Requirement: Space Accounting
The CLI SHALL report, for `--measure PATH`, the file count and logical bytes, the allocated bytes as `du` counts them, and the private (unshared) bytes where the filesystem reports them.

#### Scenario: Fresh clone
- **WHEN** a freshly cloned tree on APFS is measured
- **THEN** allocated bytes match the source while private bytes are 0

## MODIFIED Requirements

### Requirement: Graceful Error Handling
The CLI SHALL handle errors gracefully, displaying user-friendly messages to stderr and exiting with a documented status instead of panicking. With `--json` it SHALL also print `{"ok":false,"error":{...}}` on stdout, naming the error kind and path.

#### Scenario: Invalid source directory
- **WHEN** source directory does not exist
- **THEN** displays "Error: Source directory does not exist" and exits with code 1

#### Scenario: Permission denied
- **WHEN** a source entry cannot be read or the destination parent cannot be written
- **THEN** displays "Error: Permission denied: PATH" and exits with code 1

#### Scenario: Usage error
- **WHEN** an unknown option or the removed `--force` is given
- **THEN** explains the problem and exits with code 2

### Requirement: Input Validation
The CLI SHALL validate its inputs before creating anything. The source must exist and be a directory. The destination parent must exist. The destination must not exist. The destination must not lie inside the source.

#### Scenario: Source validation
- **WHEN** source is provided
- **THEN** verifies it exists and is a directory

#### Scenario: Destination inside source
- **WHEN** the destination lies inside the source, including through an aliased path such as an APFS firmlink
- **THEN** the CLI exits 1 without creating anything

### Requirement: Dry Run Mode
The CLI SHALL support `--dry-run` (`-n`) to validate the inputs and report the plan, including whether the destination is on another filesystem, without creating or writing any file.

#### Scenario: Dry run
- **WHEN** --dry-run or -n flag is used
- **THEN** displays what would be cloned, and the destination parent's entries are unchanged

### Requirement: Quiet Mode
The CLI SHALL support a --quiet (-q) flag to suppress non-error output to stdout.

#### Scenario: Quiet dry run
- **WHEN** --quiet or -q and --dry-run are used
- **THEN** no output to stdout

#### Scenario: Quiet overrides verbose
- **WHEN** --quiet and --verbose are both used
- **THEN** suppresses verbose messages

## REMOVED Requirements

### Requirement: Confirmation Prompts
**Reason**: Overwriting merged the new tree into the existing destination non-atomically and could silently replace files. The CLI now never overwrites.
**Migration**: Choose a fresh destination, or remove the existing one explicitly before cloning.
