# Project Context

## Purpose
Provide a library and command-line utility that clone directory trees with copy-on-write, with explicit fallback, atomic publication and owned cleanup.

## Tech Stack
- Rust, no crate dependencies (platform calls declared in `src/sys.rs`)

## Project Conventions

### Code Style
- Follow Rust standard formatting with rustfmt
- Use clippy for linting
- Snake_case for functions and variables, PascalCase for types

### Architecture Patterns
- Library (`src/lib.rs`) with the CLI (`src/main.rs`) as its first consumer
- Dependency-free argument parsing

### Testing Strategy
- Unit tests for core functionality
- Integration tests for CLI behavior
- Use cargo test

### Git Workflow
- Feature branches
- Conventional commit messages
- Pull requests for changes

## Domain Context
File system operations, directory cloning utilities.

## Important Constraints
- macOS (APFS `clonefile`) and Linux (`FICLONE`); no silent byte copies
- A destination is never overwritten or merged into

## External Dependencies
- None