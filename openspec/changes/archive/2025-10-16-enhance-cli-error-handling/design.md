## Context
The initial refinement added basic error handling and validation, but the review showed incomplete coverage for permission errors and validation depth. This change addresses those gaps to achieve full spec compliance and robustness.

## Goals / Non-Goals
- Goals: Specific error messages, pre-flight permission checks, comprehensive tests
- Non-Goals: New features, breaking changes, performance optimizations

## Decisions
- Error mapping: Inspect clonedir_lib error strings for keywords like "permission" to provide specific messages.
- Permission check: Use std::fs::File::create() on a temp file in destination parent to test write access.
- Test strategy: Use assert_cmd for integration tests to simulate CLI runs.

## Risks / Trade-offs
- Permission test may create temp files; mitigated by immediate deletion.
- Error string matching is heuristic; better if lib provides error types.

## Migration Plan
No migration; additive improvements.

## Open Questions
- How to handle lib error types if exposed in future versions?