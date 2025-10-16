## Context
The CLI is functional but lacks short options common in Rust tools, making it less user-friendly for command-line workflows.

## Goals / Non-Goals
- Goals: Add shorts, improve help
- Non-Goals: New features, breaking changes

## Decisions
- Short mappings: -v verbose, -f force, -n dry-run (standard conventions)
- Help: Use long_about for examples

## Risks / Trade-offs
- Slight code verbosity; improves UX

## Migration Plan
No migration; additive

## Open Questions
None