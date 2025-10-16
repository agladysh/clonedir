## Context
For automation, suppressing info output is useful to avoid cluttering logs or pipes.

## Goals / Non-Goals
- Goals: Add -q/--quiet, suppress stdout
- Non-Goals: Change error handling

## Decisions
- Short: -q
- Behavior: Suppress dry-run, prompt, verbose; assume no for prompt in quiet mode
- Precedence: Quiet overrides verbose

## Risks / Trade-offs
- Prompt suppression assumes no; may surprise users

## Migration Plan
No migration

## Open Questions
Should quiet auto-enable force? (Decided no)