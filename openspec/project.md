# Project Context

## Purpose
Create a command-line utility that wraps the clonedir_lib Rust library to provide an easy-to-use interface for cloning directories.

## Tech Stack
- Rust
- clonedir_lib crate

## Project Conventions

### Code Style
- Follow Rust standard formatting with rustfmt
- Use clippy for linting
- Snake_case for functions and variables, PascalCase for types

### Architecture Patterns
- CLI application using clap for argument parsing
- Library wrapper pattern

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
- Must wrap existing clonedir_lib API
- Cross-platform compatibility

## External Dependencies
- clonedir_lib
- clap (for CLI)