## Why
The rigorous review of the implementation against the spec revealed gaps in error handling specificity (e.g., generic messages for permission denied) and validation depth (e.g., no pre-check for destination write permissions). These issues reduce user experience, safety, and spec compliance, potentially leading to confusing failures or runtime errors.

## What Changes
- Enhance error messages: Map library errors to user-friendly messages, specifically handling permission denied scenarios.
- Add destination permission validation: Check write access to destination parent before cloning.
- Expand tests: Add integration tests for CLI behavior, error scenarios, and prompt handling.
- Minor refinements: Ensure full path normalization and improve error specificity.

## Impact
- Affected specs: cli (MODIFIED requirements for error handling and validation)
- Affected code: src/main.rs (error handling, validation), new integration tests
- No breaking changes; enhances robustness without altering existing behavior