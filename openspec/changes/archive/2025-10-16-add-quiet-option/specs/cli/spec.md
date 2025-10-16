## ADDED Requirements

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