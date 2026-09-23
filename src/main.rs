use clonedir::{Error, Fallback, Options, Receipt, Usage};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::Ordering;
use std::time::Duration;

const USAGE: &str = "\
Clone a directory tree using copy-on-write.

Usage:
  clonedir [OPTIONS] SOURCE DESTINATION
  clonedir --sweep [--json] PARENT
  clonedir --measure [--json] PATH...

DESTINATION must not exist; it appears only once complete. Symlinks are kept
verbatim, hard links become independent files, and FIFOs, sockets, devices and
nested mount points are refused. Nothing is byte-copied unless --allow-copy is
given. If the source changes while it is being cloned, the copy is discarded
(exit 3).

Options:
  -v, --verbose        Report what was done
  -n, --dry-run        Check the inputs and report the plan without creating anything
  -q, --quiet          Suppress non-error output (overrides --verbose)
      --json           Print a JSON receipt (or error) on stdout
      --allow-copy     Copy bytes where cloning is impossible (another volume or filesystem)
      --min-free SIZE  Refuse to proceed below SIZE available (default 256M; K/M/G suffixes)
      --keep-failed    Keep a failed stage for inspection instead of removing it
      --sweep          Remove stages left in PARENT by clonedir processes that are gone
      --measure        Report files, logical, du-allocated and private (unshared) bytes
  -h, --help           Show this help
  -V, --version        Show the version

Exit status: 0 success, 1 error, 2 usage, 3 source changed, 4 cannot clone
without --allow-copy, 5 insufficient space, 6 destination exists, 130 interrupted.
";

const FORCE_REMOVED: &str = "--force was removed: clonedir never overwrites or merges into an existing \
                             destination; remove it yourself or choose a new path";

#[derive(Debug, Default)]
struct Args {
    verbose: bool,
    dry_run: bool,
    quiet: bool,
    json: bool,
    allow_copy: bool,
    keep_failed: bool,
    sweep: bool,
    measure: bool,
    min_free: Option<u64>,
    paths: Vec<PathBuf>,
}

fn parse_size(text: &str) -> Option<u64> {
    let (digits, shift) = match text.chars().last()?.to_ascii_uppercase() {
        'K' => (&text[..text.len() - 1], 10),
        'M' => (&text[..text.len() - 1], 20),
        'G' => (&text[..text.len() - 1], 30),
        _ => (text, 0),
    };
    digits.parse::<u64>().ok()?.checked_mul(1u64 << shift)
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut rest = argv.iter();
    let mut only_paths = false;
    while let Some(arg) = rest.next() {
        if only_paths || !arg.starts_with('-') || arg == "-" {
            args.paths.push(PathBuf::from(arg));
            continue;
        }
        match arg.as_str() {
            "--" => only_paths = true,
            "-v" | "--verbose" => args.verbose = true,
            "-n" | "--dry-run" => args.dry_run = true,
            "-q" | "--quiet" => args.quiet = true,
            "--json" => args.json = true,
            "--allow-copy" => args.allow_copy = true,
            "--keep-failed" => args.keep_failed = true,
            "--sweep" => args.sweep = true,
            "--measure" => args.measure = true,
            "--min-free" => {
                let value = rest.next().ok_or("--min-free needs a size")?;
                args.min_free =
                    Some(parse_size(value).ok_or_else(|| format!("invalid size: {value}"))?);
            }
            "-f" | "--force" => return Err(FORCE_REMOVED.into()),
            // Short flags may be combined, as in -vn.
            short if !short.starts_with("--") && short.len() > 2 => {
                for c in short[1..].chars() {
                    match c {
                        'v' => args.verbose = true,
                        'n' => args.dry_run = true,
                        'q' => args.quiet = true,
                        'f' => return Err(FORCE_REMOVED.into()),
                        _ => return Err(format!("unknown option: -{c}")),
                    }
                }
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }
    Ok(args)
}

// --------------------------------------------------------------------- JSON

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_path(path: &Path) -> String {
    json_string(&path.to_string_lossy())
}

fn receipt_json(r: &Receipt) -> String {
    let swept: Vec<String> = r.swept_stages.iter().map(|p| json_path(p)).collect();
    format!(
        "{{\"ok\":true,\"source\":{},\"destination\":{},\"filesystem\":{},\"directories\":{},\
         \"regular_files\":{},\"symlinks\":{},\"cloned_files\":{},\"copied_files\":{},\"logical_bytes\":{},\
         \"copied_bytes\":{},\"hardlinked_files\":{},\"swept_stages\":[{}],\"available_before\":{},\
         \"available_after\":{}}}",
        json_path(&r.source),
        json_path(&r.destination),
        json_string(&r.filesystem),
        r.directories,
        r.regular_files,
        r.symlinks,
        r.cloned_files,
        r.copied_files,
        r.logical_bytes,
        r.copied_bytes,
        r.hardlinked_files,
        swept.join(","),
        r.available_before,
        r.available_after,
    )
}

fn error_json(e: &Error) -> String {
    let opt = |p: &Option<PathBuf>| p.as_deref().map(json_path).unwrap_or_else(|| "null".into());
    format!(
        "{{\"ok\":false,\"error\":{{\"kind\":{},\"path\":{},\"message\":{},\"kept_stage\":{}}}}}",
        json_string(&format!("{:?}", e.kind)),
        opt(&e.path),
        json_string(&e.detail),
        opt(&e.kept_stage),
    )
}

fn usage_json(path: &Path, u: &Usage) -> String {
    format!(
        "{{\"path\":{},\"files\":{},\"logical_bytes\":{},\"allocated_bytes\":{},\"private_bytes\":{}}}",
        json_path(path),
        u.files,
        u.logical_bytes,
        u.allocated_bytes,
        u.private_bytes
            .map(|b| b.to_string())
            .unwrap_or_else(|| "null".into()),
    )
}

// --------------------------------------------------------------------- main

fn fail(args: &Args, error: &Error) -> ! {
    if args.json {
        println!("{}", error_json(error));
    }
    let message = match (error.kind, error.path.as_deref()) {
        (clonedir::ErrorKind::InvalidSource, _) if error.detail.contains("does not exist") => {
            "Error: Source directory does not exist".to_string()
        }
        (clonedir::ErrorKind::PermissionDenied, Some(p)) => {
            format!("Error: Permission denied: {}", p.display())
        }
        _ => format!("Error: {error}"),
    };
    eprintln!("{message}");
    if let Some(stage) = &error.kept_stage {
        eprintln!("Kept stage: {}", stage.display());
    }
    process::exit(error.exit_code());
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return;
    }
    if argv.iter().any(|a| a == "-V" || a == "--version") {
        println!("clonedir {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let args = parse(&argv).unwrap_or_else(|message| {
        eprintln!("Error: {message}\n\n{USAGE}");
        process::exit(2);
    });
    let say = |text: String| {
        if !args.quiet && !args.json {
            println!("{text}");
        }
    };

    if args.measure {
        if args.paths.is_empty() {
            eprintln!("Error: --measure needs at least one PATH");
            process::exit(2);
        }
        for path in &args.paths {
            match clonedir::measure(path) {
                Ok(u) if args.json => println!("{}", usage_json(path, &u)),
                Ok(u) => say(format!(
                    "{}: {} files, {} logical bytes, {} allocated (du), {} private",
                    path.display(),
                    u.files,
                    u.logical_bytes,
                    u.allocated_bytes,
                    u.private_bytes
                        .map(|b| b.to_string())
                        .unwrap_or_else(|| "unknown".into())
                )),
                Err(e) => {
                    eprintln!("Error: {}: {e}", path.display());
                    process::exit(1);
                }
            }
        }
        return;
    }

    if args.sweep {
        let [parent] = args.paths.as_slice() else {
            eprintln!("Error: --sweep needs exactly one PARENT");
            process::exit(2);
        };
        match clonedir::sweep_stale_stages(parent) {
            Ok(removed) if args.json => {
                let list: Vec<String> = removed.iter().map(|p| json_path(p)).collect();
                println!("{{\"ok\":true,\"removed\":[{}]}}", list.join(","));
            }
            Ok(removed) => {
                for p in &removed {
                    say(format!("Removed stale stage {}", p.display()));
                }
            }
            Err(e) => {
                eprintln!("Error: {}: {e}", parent.display());
                process::exit(1);
            }
        }
        return;
    }

    let [source, destination] = args.paths.as_slice() else {
        eprintln!("Error: expected SOURCE and DESTINATION\n\n{USAGE}");
        process::exit(2);
    };

    clonedir::install_interrupt_handler();
    // Test hook: stall after N entries until interrupted, so tests can signal
    // or kill a run at a known point. Not a user option.
    let stall_after: Option<u64> = std::env::var("CLONEDIR_TEST_STALL_AFTER")
        .ok()
        .and_then(|v| v.parse().ok());
    let stall = move |entries: u64| {
        if stall_after == Some(entries) {
            while !clonedir::INTERRUPTED.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    let mut options = Options {
        fallback: if args.allow_copy {
            Fallback::Copy
        } else {
            Fallback::Fail
        },
        cancel: Some(&clonedir::INTERRUPTED),
        keep_failed_stage: args.keep_failed,
        ..Options::default()
    };
    if let Some(min_free) = args.min_free {
        options.min_free_bytes = min_free;
    }
    if stall_after.is_some() {
        options.on_entry = Some(&stall);
    }

    if args.dry_run {
        match clonedir::plan(source, destination, &options) {
            Ok(plan) if args.json => println!(
                "{{\"ok\":true,\"dry_run\":true,\"source\":{},\"destination\":{},\"cross_filesystem\":{},\
                 \"filesystem\":{},\"available_bytes\":{}}}",
                json_path(&plan.source),
                json_path(&plan.destination),
                plan.cross_filesystem,
                json_string(&plan.filesystem.type_name),
                plan.filesystem.available_bytes
            ),
            Ok(plan) => say(format!(
                "Dry run: Would clone {} to {}",
                plan.source.display(),
                plan.destination.display()
            )),
            Err(e) => fail(&args, &e),
        }
        return;
    }

    if args.verbose && !args.quiet && !args.json {
        println!("Cloning {} to {}", source.display(), destination.display());
    }
    match clonedir::clone_tree(source, destination, &options) {
        Ok(receipt) if args.json => println!("{}", receipt_json(&receipt)),
        Ok(r) => {
            if args.verbose {
                say(format!(
                    "Clone completed successfully: {} directories, {} files ({} cloned, {} copied, {} bytes \
                     copied of {} logical), {} symlinks",
                    r.directories,
                    r.regular_files,
                    r.cloned_files,
                    r.copied_files,
                    r.copied_bytes,
                    r.logical_bytes,
                    r.symlinks
                ));
            }
        }
        Err(e) => fail(&args, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_paths_and_flags() {
        let a = parse(&argv(&[
            "-vn",
            "--json",
            "--min-free",
            "1G",
            "/src",
            "/dst",
        ]))
        .unwrap();
        assert!(a.verbose && a.dry_run && a.json && !a.quiet && !a.allow_copy);
        assert_eq!(a.min_free, Some(1 << 30));
        assert_eq!(a.paths, vec![PathBuf::from("/src"), PathBuf::from("/dst")]);
    }

    #[test]
    fn force_is_refused_with_a_reason() {
        assert!(
            parse(&argv(&["-f", "/a", "/b"]))
                .unwrap_err()
                .contains("never overwrites")
        );
        assert!(parse(&argv(&["--force", "/a", "/b"])).is_err());
    }

    #[test]
    fn double_dash_ends_options() {
        let a = parse(&argv(&["--", "-odd", "/dst"])).unwrap();
        assert_eq!(a.paths, vec![PathBuf::from("-odd"), PathBuf::from("/dst")]);
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_string("a\"b\\c\n\u{1}"), "\"a\\\"b\\\\c\\n\\u0001\"");
    }
}
