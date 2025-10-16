## Why
The CLI currently outputs info messages (e.g., dry-run preview, prompts) to stdout, which can clutter output in scripts or automation. Adding --quiet allows suppressing non-error output for cleaner, silent operation.

## What Changes
- Add --quiet (-q) flag to suppress stdout messages (dry-run, prompts, verbose).
- Ensure errors still go to stderr.
- Quiet takes precedence over verbose.

## Impact
- Affected specs: cli (ADDED requirement for quiet mode)
- Affected code: src/main.rs (add flag, conditional output)
- No breaking changes; enhances usability for automation