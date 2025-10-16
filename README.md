# clonedir

A command-line utility for cloning directories using copy-on-write where possible.

## Description

`clonedir` is a Rust-based CLI tool that wraps the `clonedir_lib` library to provide an efficient way to clone directories. It leverages copy-on-write mechanisms on supported filesystems to minimize disk usage and improve performance.

## Features

- **Copy-on-Write Cloning**: Uses filesystem-level copy-on-write for efficient directory cloning
- **Cross-Platform**: Works on Linux, macOS, and Windows
- **Verbose Output**: Optional detailed progress reporting
- **Dry Run Mode**: Preview operations without making changes
- **Force Overwrite**: Skip confirmation prompts for existing destinations
- **Quiet Mode**: Suppress non-error output
- **Robust Error Handling**: User-friendly error messages and proper exit codes

## Installation

### From Crates.io (when published)

```bash
cargo install clonedir
```

### Build from Source

1. Clone the repository:
   ```bash
   git clone <repository-url>
   cd clonedir
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

3. The binary will be available at `target/release/clonedir`

## Usage

### Basic Usage

Clone a directory from source to destination:

```bash
clonedir /path/to/source /path/to/destination
```

### Options

- `-v, --verbose`: Enable verbose output showing progress
- `-n, --dry-run`: Perform a dry run without making changes
- `-f, --force`: Force overwrite without confirmation
- `-q, --quiet`: Suppress non-error output

### Examples

Clone with verbose output:
```bash
clonedir -v /src/dir /dst/dir
```

Dry run to preview:
```bash
clonedir -n /src/dir /dst/dir
```

Force overwrite existing destination:
```bash
clonedir -f /src/dir /dst/dir
```

Quiet mode (no output except errors):
```bash
clonedir -q /src/dir /dst/dir
```

## Building

To build the project:

```bash
cargo build --release
```

## Testing

Run the test suite:

```bash
cargo test
```

## License

This project is licensed under the terms specified in the LICENSE file.

## Contributing

Contributions are welcome! Please see the project's contribution guidelines.