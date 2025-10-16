## MODIFIED Requirements

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