## Why
The current AI-generated stub implementation lacks proper error handling, input validation, and user experience features, making it unreliable and hard to use. This refinement ensures robustness, safety, and usability for the directory cloning utility.

## What Changes
- Add graceful error handling with user-friendly messages instead of panics
- Implement basic path validation for source and destination
- Add optional verbose output for progress tracking
- Include confirmation prompts for overwriting existing destinations
- Enhance CLI with additional flags (e.g., --force, --dry-run)
- Add unit and integration tests as per project conventions

## Impact
- Affected specs: None (new CLI enhancements)
- Affected code: src/main.rs, new test files
- No breaking changes; improves reliability without altering core functionality