use clap::Parser;
use std::fmt;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;

#[derive(Debug)]
enum CliError {
    Io(std::io::Error),
    SourceNotDirectory,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CliError::Io(e) => write!(f, "IO error: {}", e),
            CliError::SourceNotDirectory => write!(f, "Source is not a directory"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e)
    }
}

#[derive(Parser)]
#[command(name = "clonedir")]
#[command(about = "Clone a directory using copy-on-write where possible")]
#[command(long_about = "Clone a directory using copy-on-write where possible.

Examples:
  clonedir /src /dst
  clonedir -v /src /dst
  clonedir -n /src /dst  # Dry run
  clonedir -f /src /dst  # Force overwrite")]
struct Args {
    /// Source directory to clone
    from: PathBuf,
    /// Destination directory
    to: PathBuf,
    /// Enable verbose output
    #[arg(short = 'v', long)]
    verbose: bool,
    /// Perform a dry run without making changes
    #[arg(short = 'n', long)]
    dry_run: bool,
    /// Force overwrite without confirmation
    #[arg(short = 'f', long)]
    force: bool,
    /// Suppress non-error output
    #[arg(short = 'q', long)]
    quiet: bool,
}

fn main() {
    let args = Args::parse();

    // Normalize paths
    let from = match args.from.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("Error: Source directory does not exist");
            process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: {}", CliError::Io(e));
            process::exit(1);
        }
    };
    if !from.is_dir() {
        eprintln!("Error: {}", CliError::SourceNotDirectory);
        process::exit(1);
    }

    let to = args.to;

    // Validate destination
    if let Some(parent) = to.parent() {
        if !parent.exists() {
            eprintln!("Error: Parent directory of destination does not exist");
            process::exit(1);
        }
        // Check write permission
        let temp_path = parent.join(".clonedir_temp_check");
        match std::fs::File::create(&temp_path) {
            Ok(file) => {
                drop(file);
                let _ = std::fs::remove_file(&temp_path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("Error: Permission denied for destination");
                process::exit(1);
            }
            Err(e) => {
                eprintln!("Error: {}", CliError::Io(e));
                process::exit(1);
            }
        }
    }

    if to.exists() && !args.force {
        if args.quiet {
            // Assume no in quiet mode
            return;
        } else {
            print!(
                "Destination {} already exists. Overwrite? (y/N): ",
                to.display()
            );
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            if !input.trim().eq_ignore_ascii_case("y") {
                if !args.quiet {
                    println!("Aborted.");
                }
                return;
            }
        }
    }

    if args.dry_run {
        if !args.quiet {
            println!(
                "Dry run: Would clone {} to {}",
                from.display(),
                to.display()
            );
        }
        return;
    }

    if args.verbose && !args.quiet {
        println!("Cloning {} to {}", from.display(), to.display());
    }

    if let Err(e) = clonedir_lib::clonedir(&from, &to) {
        let msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
            "Error: Permission denied for destination"
        } else {
            &format!("Error: Clone error: {}", e)
        };
        eprintln!("{}", msg);
        process::exit(1);
    }

    if args.verbose && !args.quiet {
        println!("Clone completed successfully");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_parse_basic_args() {
        let args = Args::try_parse_from(["clonedir", "/src", "/dst"]).unwrap();
        assert_eq!(args.from, PathBuf::from("/src"));
        assert_eq!(args.to, PathBuf::from("/dst"));
        assert!(!args.verbose);
        assert!(!args.dry_run);
        assert!(!args.force);
        assert!(!args.quiet);
    }

    #[test]
    fn test_parse_with_flags() {
        let args = Args::try_parse_from([
            "clonedir",
            "/src",
            "/dst",
            "--verbose",
            "--dry-run",
            "--force",
        ])
        .unwrap();
        assert!(args.verbose);
        assert!(args.dry_run);
        assert!(args.force);
    }

    #[test]
    fn test_parse_with_shorts() {
        let args =
            Args::try_parse_from(["clonedir", "/src", "/dst", "-v", "-n", "-f", "-q"]).unwrap();
        assert!(args.verbose);
        assert!(args.dry_run);
        assert!(args.force);
        assert!(args.quiet);
    }

    #[test]
    fn test_cli_error_display() {
        assert_eq!(
            format!("{}", CliError::SourceNotDirectory),
            "Source is not a directory"
        );
    }
}
