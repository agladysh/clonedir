## Why
The current CLI options lack short flags and detailed help, reducing usability for power users and deviating from idiomatic Rust CLI patterns (e.g., -v for --verbose). This makes the tool less ergonomic compared to standard tools like `cp` or `rsync`.

## What Changes
- Add short flags: -v (--verbose), -f (--force), -n (--dry-run).
- Customize help text with usage examples.
- Ensure backward compatibility (long flags still work).

## Impact
- Affected specs: cli (MODIFIED requirements for verbose, force, dry-run to include short options)
- Affected code: src/main.rs (Args struct), help output
- No breaking changes; improves UX without altering behavior