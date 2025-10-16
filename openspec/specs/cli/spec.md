## Purpose
Create a command-line utility that wraps the clonedir_lib Rust library to provide an easy-to-use interface for cloning directories.
## Requirements
### Requirement: Graceful Error Handling
The CLI SHALL handle errors gracefully, displaying user-friendly messages to stderr and exiting with appropriate codes instead of panicking.

#### Scenario: Invalid source directory
- **WHEN** source directory does not exist
- **THEN** displays "Error: Source directory does not exist" and exits with code 1

#### Scenario: Permission denied
- **WHEN** destination cannot be written due to permissions
- **THEN** displays "Error: Permission denied for destination" and exits with code 1

### Requirement: Input Validation
The CLI SHALL validate input paths before attempting to clone.

#### Scenario: Source validation
- **WHEN** source is provided
- **THEN** verifies it exists and is a directory

#### Scenario: Destination validation
- **WHEN** destination is provided
- **THEN** checks write permissions and normalizes path

### Requirement: Verbose Output
The CLI SHALL support a --verbose flag for detailed operation feedback.

#### Scenario: Verbose clone
- **WHEN** --verbose flag is used
- **THEN** displays progress messages during cloning

### Requirement: Confirmation Prompts
The CLI SHALL prompt for confirmation before overwriting existing destinations unless --force is specified.

#### Scenario: Overwrite prompt
- **WHEN** destination exists and --force not used
- **THEN** prompts user for confirmation

### Requirement: Dry Run Mode
The CLI SHALL support --dry-run to preview operations without executing.

#### Scenario: Dry run
- **WHEN** --dry-run flag is used
- **THEN** displays what would be cloned without making changes

