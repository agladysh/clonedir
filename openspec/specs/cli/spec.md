## Purpose
Create a command-line utility that wraps the clonedir_lib Rust library to provide an easy-to-use interface for cloning directories.
## Requirements
### Requirement: Graceful Error Handling
The CLI SHALL handle errors gracefully, displaying user-friendly messages to stderr and exiting with appropriate codes instead of panicking. Error messages SHALL be specific, mapping library errors to clear user feedback.

#### Scenario: Invalid source directory
- **WHEN** source directory does not exist
- **THEN** displays "Error: Source directory does not exist" and exits with code 1

#### Scenario: Permission denied
- **WHEN** destination cannot be written due to permissions
- **THEN** displays "Error: Permission denied for destination" and exits with code 1

#### Scenario: Library error mapping
- **WHEN** clonedir_lib fails with a mappable error
- **THEN** displays specific message (e.g., "Error: Permission denied") instead of generic lib error

### Requirement: Input Validation
The CLI SHALL validate input paths before attempting to clone, including write permissions for destination.

#### Scenario: Source validation
- **WHEN** source is provided
- **THEN** verifies it exists and is a directory

#### Scenario: Destination validation
- **WHEN** destination is provided
- **THEN** checks parent exists, write permissions, and normalizes path

#### Scenario: Permission check failure
- **WHEN** destination parent lacks write permissions
- **THEN** displays "Error: Permission denied for destination" and exits with code 1

### Requirement: Verbose Output
The CLI SHALL support a --verbose (-v) flag for detailed operation feedback.

#### Scenario: Verbose clone
- **WHEN** --verbose or -v flag is used
- **THEN** displays progress messages during cloning

### Requirement: Confirmation Prompts
The CLI SHALL prompt for confirmation before overwriting existing destinations unless --force (-f) is specified.

#### Scenario: Overwrite prompt
- **WHEN** destination exists and --force/-f not used
- **THEN** prompts user for confirmation

### Requirement: Dry Run Mode
The CLI SHALL support --dry-run (-n) to preview operations without executing.

#### Scenario: Dry run
- **WHEN** --dry-run or -n flag is used
- **THEN** displays what would be cloned without making changes

### Requirement: Quiet Mode
The CLI SHALL support a --quiet (-q) flag to suppress non-error output to stdout.

#### Scenario: Quiet dry run
- **WHEN** --quiet or -q and --dry-run are used
- **THEN** no output to stdout

#### Scenario: Quiet with prompt
- **WHEN** --quiet and destination exists
- **THEN** assumes no overwrite without prompting

#### Scenario: Quiet overrides verbose
- **WHEN** --quiet and --verbose are both used
- **THEN** suppresses verbose messages

