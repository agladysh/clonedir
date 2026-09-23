//! The command line as adopters call it: JSON receipts, exit statuses,
//! interruption by signal, a killed run and its sweep, racing processes.

mod common;

use common::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_clonedir");

fn run(args: &[&Path], flags: &[&str]) -> Output {
    Command::new(BIN).args(flags).args(args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn small_tree(s: &Scratch) -> std::path::PathBuf {
    let src = s.path("src");
    for i in 0..40 {
        write(
            &src.join(format!("d{}/f{i}", i % 5)),
            format!("file {i}").as_bytes(),
        );
    }
    std::os::unix::fs::symlink("d0/f0", src.join("link")).unwrap();
    src
}

#[test]
fn json_receipt_and_distinct_exit_statuses() {
    let s = Scratch::new("cli-json");
    let src = small_tree(&s);
    let ok = run(&[&src, &s.path("dst")], &["--json", "--min-free", "0"]);
    assert_eq!(ok.status.code(), Some(0));
    let out = stdout(&ok);
    for field in [
        "\"ok\":true",
        "\"cloned_files\":40",
        "\"copied_bytes\":0",
        "\"symlinks\":1",
        "\"directories\":6",
    ] {
        assert!(out.contains(field), "{field} in {out}");
    }

    let exists = run(&[&src, &s.path("dst")], &["--json", "--min-free", "0"]);
    assert_eq!(exists.status.code(), Some(6));
    assert!(stdout(&exists).contains("\"kind\":\"DestinationExists\""));

    let missing = run(&[&s.path("nope"), &s.path("x")], &["--min-free", "0"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Source directory does not exist"));

    let force = run(&[&src, &s.path("y")], &["-f"]);
    assert_eq!(force.status.code(), Some(2));
    assert!(!s.path("y").exists());

    let floor = run(&[&src, &s.path("z")], &["--json", "--min-free", "1000000G"]);
    assert_eq!(floor.status.code(), Some(5));
    assert!(!s.path("z").exists());
}

#[test]
fn dry_run_creates_nothing_and_writes_no_probe() {
    let s = Scratch::new("cli-dry");
    let src = small_tree(&s);
    // 0.1 truncated a file of this name in the destination parent.
    write(&s.path(".clonedir_temp_check"), b"not yours");
    let before = s.names();
    let o = run(
        &[&src, &s.path("dst")],
        &["-n", "--json", "--min-free", "0"],
    );
    assert_eq!(o.status.code(), Some(0));
    assert!(
        stdout(&o).contains("\"dry_run\":true")
            && stdout(&o).contains("\"cross_filesystem\":false")
    );
    assert_eq!(s.names(), before);
    assert_eq!(
        fs::read(s.path(".clonedir_temp_check")).unwrap(),
        b"not yours"
    );
}

/// Start a run that stalls after `after` entries, and wait until its stage
/// holds a partial tree.
fn stalled_run(s: &Scratch, src: &Path, after: u64) -> std::process::Child {
    let mut child = Command::new(BIN)
        .args(["--min-free", "0"])
        .arg(src)
        .arg(s.path("dst"))
        .env("CLONEDIR_TEST_STALL_AFTER", after.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !stages(&s.0).iter().any(|st| st.join("tree/d0").exists()) {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("stage never appeared");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Give it a moment to reach the stall point.
    std::thread::sleep(Duration::from_millis(100));
    child
}

#[test]
fn sigterm_mid_run_removes_the_partial_stage() {
    let s = Scratch::new("cli-term");
    let src = small_tree(&s);
    let mut child = stalled_run(&s, &src, 5);
    signal(child.id(), 15);
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(130));
    assert_eq!(s.names(), vec!["src"], "no destination, no stage");
}

#[test]
fn a_killed_run_leaves_an_owned_stage_that_sweep_and_the_next_run_remove() {
    let s = Scratch::new("cli-kill");
    let src = small_tree(&s);
    let mut child = stalled_run(&s, &src, 5);
    signal(child.id(), 9);
    child.wait().unwrap();
    let left = stages(&s.0);
    assert_eq!(left.len(), 1, "SIGKILL cannot clean up");
    assert!(
        fs::read_to_string(left[0].join("owner"))
            .unwrap()
            .contains(&format!("pid={}", child.id()))
    );
    assert!(!s.path("dst").exists(), "nothing was published");

    let swept = run(&[&s.0], &["--sweep", "--json"]);
    assert_eq!(swept.status.code(), Some(0));
    assert!(
        stdout(&swept).contains(&*left[0].to_string_lossy()),
        "{}",
        stdout(&swept)
    );
    assert_eq!(s.names(), vec!["src"]);

    // Same again, but let the next clone into this parent do the sweeping.
    let mut child = stalled_run(&s, &src, 5);
    signal(child.id(), 9);
    child.wait().unwrap();
    let ok = run(&[&src, &s.path("dst")], &["--json", "--min-free", "0"]);
    assert_eq!(ok.status.code(), Some(0));
    assert!(
        stdout(&ok).contains("\"swept_stages\":[\""),
        "{}",
        stdout(&ok)
    );
    assert_eq!(s.names(), vec!["dst", "src"]);
    assert_eq!(listing(&src), listing(&s.path("dst")));
}

#[test]
fn racing_processes_publish_one_destination() {
    let s = Scratch::new("cli-race");
    let src = small_tree(&s);
    let children: Vec<_> = (0..6)
        .map(|_| {
            Command::new(BIN)
                .args(["--min-free", "0"])
                .arg(&src)
                .arg(s.path("dst"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    let codes: Vec<i32> = children
        .into_iter()
        .map(|mut c| c.wait().unwrap().code().unwrap())
        .collect();
    assert_eq!(codes.iter().filter(|&&c| c == 0).count(), 1, "{codes:?}");
    assert!(codes.iter().all(|&c| c == 0 || c == 6), "{codes:?}");
    assert_eq!(listing(&src), listing(&s.path("dst")));
    assert!(stages(&s.0).is_empty());
}

#[test]
fn measure_reports_du_and_private_bytes_separately() {
    let s = Scratch::new("cli-measure");
    write(&s.path("src/blob"), &noise(1 << 20, 9));
    assert_eq!(
        run(&[&s.path("src"), &s.path("dst")], &["--min-free", "0"])
            .status
            .code(),
        Some(0)
    );
    let o = run(&[&s.path("dst")], &["--measure", "--json"]);
    let out = stdout(&o);
    assert!(
        out.contains("\"files\":1") && out.contains("\"logical_bytes\":1048576"),
        "{out}"
    );
    #[cfg(target_os = "macos")]
    assert!(
        out.contains("\"private_bytes\":0"),
        "a fresh clone owns nothing: {out}"
    );
}

#[test]
fn a_kept_failed_stage_survives_the_retry_and_sweep() {
    let s = Scratch::new("cli-keep");
    let src = small_tree(&s);
    let fifo = src.join("zz-fifo");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let failed = run(
        &[&src, &s.path("dst")],
        &["--json", "--keep-failed", "--min-free", "0"],
    );
    assert_eq!(failed.status.code(), Some(1));
    let kept = stages(&s.0);
    assert_eq!(kept.len(), 1, "{}", stdout(&failed));

    fs::remove_file(&fifo).unwrap();
    let retry = run(&[&src, &s.path("dst")], &["--json", "--min-free", "0"]);
    assert_eq!(retry.status.code(), Some(0), "{}", stdout(&retry));
    assert!(
        stdout(&retry).contains("\"swept_stages\":[]"),
        "{}",
        stdout(&retry)
    );
    assert_eq!(run(&[&s.0], &["--sweep", "--json"]).status.code(), Some(0));
    assert_eq!(stages(&s.0), kept, "kept for inspection, as asked");
}
