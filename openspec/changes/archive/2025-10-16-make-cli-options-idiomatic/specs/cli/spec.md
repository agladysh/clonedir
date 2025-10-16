## MODIFIED Requirements

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