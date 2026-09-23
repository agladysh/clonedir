//! The library contract: structure, independence, refusal, cleanup,
//! concurrency, and what cloning does and does not do to physical space.

mod common;

use clonedir::{ErrorKind, Options, clone_tree, measure, sharing, sweep_stale_stages};
use common::*;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

fn opts() -> Options<'static> {
    Options {
        min_free_bytes: 0,
        ..Options::default()
    }
}

/// A small tree exercising what the 0.1 clonedir got wrong: nesting, same
/// names at different depths, empty directories, links of every kind.
fn fixture(s: &Scratch) -> std::path::PathBuf {
    let src = s.path("src");
    write(&src.join("top"), b"top");
    write(&src.join("a/b/same"), b"deep");
    write(&src.join("c/same"), b"shallow");
    write(&src.join("a/b/c/d/e/leaf"), b"leaf");
    write(&src.join("a/script"), b"#!/bin/sh\n");
    fs::set_permissions(src.join("a/script"), fs::Permissions::from_mode(0o750)).unwrap();
    write(&src.join("secret"), b"s");
    fs::set_permissions(src.join("secret"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::create_dir_all(src.join("empty/nested-empty")).unwrap();
    symlink("a/b/same", src.join("rel-file-link")).unwrap();
    symlink("a/b", src.join("rel-dir-link")).unwrap();
    symlink("/nonexistent/target", src.join("dangling")).unwrap();
    symlink(s.path("outside"), src.join("external")).unwrap();
    write(&s.path("outside"), b"external sentinel");
    fs::create_dir(src.join("readonly")).unwrap();
    write(&src.join("readonly/inside"), b"ro");
    fs::set_permissions(src.join("readonly"), fs::Permissions::from_mode(0o555)).unwrap();
    src
}

#[test]
fn preserves_structure_modes_times_and_links() {
    let s = Scratch::new("structure");
    let src = fixture(&s);
    let receipt = clone_tree(&src, &s.path("dst"), &opts()).unwrap();
    let dst = s.path("dst");

    assert_eq!(listing(&src), listing(&dst));
    assert_eq!(fs::read(dst.join("a/b/same")).unwrap(), b"deep");
    assert_eq!(fs::read(dst.join("c/same")).unwrap(), b"shallow");
    assert!(dst.join("empty/nested-empty").is_dir());
    for link in ["rel-file-link", "rel-dir-link", "dangling", "external"] {
        assert!(
            fs::symlink_metadata(dst.join(link))
                .unwrap()
                .file_type()
                .is_symlink(),
            "{link} kept as a link"
        );
        assert_eq!(
            fs::read_link(dst.join(link)).unwrap(),
            fs::read_link(src.join(link)).unwrap()
        );
    }
    for rel in [
        "a/script",
        "secret",
        "readonly",
        "a/b",
        "empty",
        "top",
        "a/b/c/d/e/leaf",
    ] {
        let (m, n) = (
            fs::symlink_metadata(src.join(rel)).unwrap(),
            fs::symlink_metadata(dst.join(rel)).unwrap(),
        );
        assert_eq!(m.mode(), n.mode(), "mode of {rel}");
        assert_eq!(
            (m.mtime(), m.mtime_nsec()),
            (n.mtime(), n.mtime_nsec()),
            "mtime of {rel}"
        );
        assert_ne!(m.ino(), n.ino(), "{rel} is its own inode");
    }
    assert_eq!(
        fs::symlink_metadata(&dst).unwrap().mode(),
        fs::symlink_metadata(&src).unwrap().mode()
    );
    assert_eq!(
        (
            receipt.directories,
            receipt.regular_files,
            receipt.symlinks,
            receipt.copied_files,
            receipt.copied_bytes
        ),
        (10, 7, 4, 0, 0)
    );
    assert_eq!(receipt.cloned_files, 7);
    assert!(stages(&s.0).is_empty());
}

#[test]
fn source_and_clone_stay_independent_after_mutation() {
    let s = Scratch::new("isolation");
    let src = fixture(&s);
    let before = listing(&src);
    clone_tree(&src, &s.path("dst"), &opts()).unwrap();
    let dst = s.path("dst");

    // Writes through the clone, including through its copied links, stay there.
    fs::write(dst.join("a/b/same"), b"changed in clone").unwrap();
    append(&dst.join("top"), b"+");
    fs::OpenOptions::new()
        .write(true)
        .open(dst.join("c/same"))
        .unwrap()
        .set_len(1)
        .unwrap();
    fs::write(dst.join("rel-file-link"), b"through clone link").unwrap();
    fs::remove_file(dst.join("a/b/c/d/e/leaf")).unwrap();
    assert_eq!(listing(&src), before, "source untouched by clone writes");
    assert_eq!(
        fs::read(s.path("outside")).unwrap(),
        b"external sentinel",
        "not a writable view of outside"
    );

    // And the reverse.
    let clone_now = listing(&dst);
    fs::write(src.join("c/same"), b"changed in source").unwrap();
    append(&src.join("a/script"), b"echo\n");
    assert_eq!(listing(&dst), clone_now, "clone untouched by source writes");
}

#[test]
fn hardlinks_become_independent_files() {
    let s = Scratch::new("hardlinks");
    let src = s.path("src");
    write(&src.join("one"), b"shared inode");
    fs::create_dir(src.join("sub")).unwrap();
    fs::hard_link(src.join("one"), src.join("sub/two")).unwrap();
    let receipt = clone_tree(&src, &s.path("dst"), &opts()).unwrap();
    assert_eq!(receipt.hardlinked_files, 1);
    let dst = s.path("dst");
    assert_ne!(
        fs::metadata(dst.join("one")).unwrap().ino(),
        fs::metadata(dst.join("sub/two")).unwrap().ino()
    );
    fs::write(dst.join("one"), b"edited").unwrap();
    assert_eq!(fs::read(dst.join("sub/two")).unwrap(), b"shared inode");
    assert_eq!(fs::read(src.join("one")).unwrap(), b"shared inode");
    assert_eq!(fs::read(src.join("sub/two")).unwrap(), b"shared inode");
}

#[test]
fn existing_destinations_are_never_touched() {
    let s = Scratch::new("exists");
    let src = fixture(&s);
    write(&s.path("file"), b"keep");
    fs::create_dir(s.path("emptydir")).unwrap();
    symlink("nowhere", s.path("deadlink")).unwrap();
    for name in ["file", "emptydir", "deadlink"] {
        let error = clone_tree(&src, &s.path(name), &opts()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::DestinationExists, "{name}");
    }
    assert_eq!(fs::read(s.path("file")).unwrap(), b"keep");
    assert_eq!(fs::read_dir(s.path("emptydir")).unwrap().count(), 0);
    assert_eq!(
        fs::read_link(s.path("deadlink")).unwrap().to_str(),
        Some("nowhere")
    );
    assert!(stages(&s.0).is_empty());
}

#[test]
fn destination_inside_source_is_refused_before_anything_is_created() {
    let s = Scratch::new("nested");
    let src = fixture(&s);
    let before = listing(&src);
    let error = clone_tree(&src, &src.join("a/copy"), &opts()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Nested);
    assert_eq!(listing(&src), before);
}

#[test]
fn special_files_are_refused_and_leave_nothing() {
    let s = Scratch::new("fifo");
    let src = s.path("src");
    write(&src.join("a/file"), b"x");
    make_fifo(&src.join("a/pipe"));
    let error = clone_tree(&src, &s.path("dst"), &opts()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::SpecialFile);
    assert_eq!(error.path.as_deref(), Some(src.join("a/pipe").as_path()));
    assert_eq!(s.names(), vec!["src"]);
}

#[test]
fn a_failed_copy_is_removed_even_with_read_only_directories_in_it() {
    if is_root() {
        return; // root reads mode-000 files, so there is no failure to observe
    }
    let s = Scratch::new("unreadable");
    let src = s.path("src");
    // Sorted order: the read-only directory is fully copied (and made
    // read-only in the stage) before the unreadable file fails.
    write(&src.join("a-readonly/inner/file"), b"x");
    fs::set_permissions(
        src.join("a-readonly/inner"),
        fs::Permissions::from_mode(0o555),
    )
    .unwrap();
    fs::set_permissions(src.join("a-readonly"), fs::Permissions::from_mode(0o555)).unwrap();
    write(&src.join("z-unreadable"), b"secret");
    fs::set_permissions(src.join("z-unreadable"), fs::Permissions::from_mode(0o000)).unwrap();

    let error = clone_tree(&src, &s.path("dst"), &opts()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::PermissionDenied);
    assert_eq!(
        error.path.as_deref(),
        Some(src.join("z-unreadable").as_path())
    );
    assert!(error.kept_stage.is_none());
    assert_eq!(s.names(), vec!["src"], "no destination and no stage");

    let error = clone_tree(
        &src,
        &s.path("dst"),
        &Options {
            keep_failed_stage: true,
            ..opts()
        },
    )
    .unwrap_err();
    let kept = error.kept_stage.expect("stage kept on request");
    assert!(kept.join("tree/a-readonly/inner/file").exists());
    assert!(!s.path("dst").exists());
    force_remove(&kept);
}

#[test]
fn cancellation_discards_the_partial_copy() {
    let s = Scratch::new("cancel");
    let src = fixture(&s);
    let cancel = AtomicBool::new(false);
    let hook = |n: u64| {
        if n >= 3 {
            cancel.store(true, Ordering::SeqCst);
        }
    };
    let options = Options {
        cancel: Some(&cancel),
        on_entry: Some(&hook),
        ..opts()
    };
    let error = clone_tree(&src, &s.path("dst"), &options).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Interrupted);
    assert_eq!(error.exit_code(), 130);
    assert_eq!(s.names(), vec!["outside", "src"]);
}

#[test]
fn a_source_changing_during_the_clone_is_detected_and_discarded() {
    // Three kinds of concurrent writer, each acting after the entry it touches
    // has already been cloned: content rewrite, chmod, and a new entry.
    type Mutation = fn(&std::path::Path);
    let mutations: [(&str, Mutation); 3] = [
        ("rewrite", |src| {
            fs::write(src.join("a/b/same"), b"rewritten").unwrap()
        }),
        ("chmod", |src| {
            fs::set_permissions(src.join("a/script"), fs::Permissions::from_mode(0o700)).unwrap()
        }),
        ("new entry", |src| {
            fs::write(src.join("a/b/c/new"), b"late").unwrap()
        }),
    ];
    for (label, mutate) in mutations {
        let s = Scratch::new("source-change");
        let src = fixture(&s);
        let done = AtomicBool::new(false);
        let src_for_hook = src.clone();
        let hook = move |n: u64| {
            if n >= 12 && !done.swap(true, Ordering::SeqCst) {
                mutate(&src_for_hook);
            }
        };
        let options = Options {
            on_entry: Some(&hook),
            ..opts()
        };
        let error = clone_tree(&src, &s.path("dst"), &options).unwrap_err();
        assert_eq!(error.kind, ErrorKind::SourceChanged, "{label}");
        assert_eq!(error.exit_code(), 3);
        assert_eq!(
            s.names(),
            vec!["outside", "src"],
            "{label}: nothing published, stage removed"
        );
        // Once the writer has stopped, the same call succeeds.
        clone_tree(&src, &s.path("dst"), &opts()).unwrap();
        assert_eq!(listing(&src), listing(&s.path("dst")), "{label}");
    }
}

#[test]
fn concurrent_clones_of_one_source_are_all_complete() {
    let s = Arc::new(Scratch::new("fanout"));
    let src = fixture(&s);
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let (s, src, barrier) = (s.clone(), src.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                clone_tree(&src, &s.path(&format!("dst{i}")), &opts()).unwrap()
            })
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap().cloned_files, 7);
    }
    let expected = listing(&src);
    for i in 0..8 {
        assert_eq!(listing(&s.path(&format!("dst{i}"))), expected);
    }
    assert!(stages(&s.0).is_empty());
}

#[test]
fn racing_clones_to_one_destination_publish_exactly_one_tree() {
    let s = Arc::new(Scratch::new("race"));
    let src = fixture(&s);
    let barrier = Arc::new(Barrier::new(8));
    let (won, lost) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (s, src, barrier, won, lost) = (
                s.clone(),
                src.clone(),
                barrier.clone(),
                won.clone(),
                lost.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                match clone_tree(&src, &s.path("dst"), &opts()) {
                    Ok(_) => won.fetch_add(1, Ordering::SeqCst),
                    Err(e) => {
                        assert_eq!(e.kind, ErrorKind::DestinationExists);
                        lost.fetch_add(1, Ordering::SeqCst)
                    }
                };
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(
        (won.load(Ordering::SeqCst), lost.load(Ordering::SeqCst)),
        (1, 7)
    );
    assert_eq!(
        listing(&s.path("dst")),
        listing(&src),
        "the one published tree is complete, not merged"
    );
    assert!(stages(&s.0).is_empty(), "losers removed their stages");
}

#[test]
fn stale_stages_are_swept_only_when_their_owner_is_gone() {
    let s = Scratch::new("sweep");
    let src = fixture(&s);
    let stage = |name: &str, owner: Option<String>| {
        let dir = s.path(&format!("{}{name}", clonedir::STAGE_PREFIX));
        write(&dir.join("tree/partial"), b"clone of source");
        if let Some(text) = owner {
            fs::write(dir.join("owner"), text).unwrap();
        }
        dir
    };
    let dead = stage(
        "dead",
        Some(format!("clonedir-stage v1\npid={}\n", dead_pid())),
    );
    let live = stage(
        "live",
        Some(format!("clonedir-stage v1\npid={}\n", std::process::id())),
    );
    let unowned = stage("unowned", None);
    let foreign = stage(
        "foreign-format",
        Some(format!("something else\npid={}\n", dead_pid())),
    );
    write(&s.path("unrelated-dir/file"), b"not ours");

    // The next clone into the same parent removes the dead one and reports it.
    let receipt = clone_tree(&src, &s.path("dst"), &opts()).unwrap();
    assert_eq!(receipt.swept_stages, vec![dead.clone()]);
    assert!(!dead.exists());
    for kept in [&live, &unowned, &foreign] {
        assert!(kept.exists(), "{} kept", kept.display());
    }
    assert!(s.path("unrelated-dir/file").exists());
    assert!(
        sweep_stale_stages(&s.0).unwrap().is_empty(),
        "nothing else is stale"
    );
}

#[test]
fn a_read_only_source_root_is_published_with_its_mode() {
    let s = Scratch::new("ro-root");
    let src = s.path("src");
    write(&src.join("sub/f"), b"f");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o555)).unwrap();
    let result = clone_tree(&src, &s.path("dst"), &opts());
    fs::set_permissions(&src, fs::Permissions::from_mode(0o755)).unwrap();
    result.unwrap();
    let dst = fs::symlink_metadata(s.path("dst")).unwrap();
    assert_eq!(dst.mode() & 0o7777, 0o555);
    assert_eq!(fs::read(s.path("dst/sub/f")).unwrap(), b"f");
    assert!(stages(&s.0).is_empty());
    fs::set_permissions(s.path("dst"), fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn a_destination_inside_the_source_through_a_firmlink_alias_is_refused() {
    let s = Scratch::new("alias");
    let src = fixture(&s);
    let alias = std::path::Path::new("/System/Volumes/Data").join(src.strip_prefix("/").unwrap());
    if !alias.is_dir() {
        return; // not on a firmlinked data volume
    }
    let error = clone_tree(&src, &alias.join("a/inner"), &opts()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Nested);
    assert!(stages(&src.join("a")).is_empty(), "nothing was created");
}

/// A dead run's stage with enough entries that concurrent sweeps overlap.
fn stale_stage(s: &Scratch) -> std::path::PathBuf {
    let dir = s.path(&format!("{}stale", clonedir::STAGE_PREFIX));
    for d in 0..20 {
        for f in 0..40 {
            write(&dir.join(format!("tree/d{d}/f{f}")), b"");
        }
    }
    fs::write(
        dir.join("owner"),
        format!("clonedir-stage v1\npid={}\n", dead_pid()),
    )
    .unwrap();
    dir
}

#[test]
fn concurrent_clones_sweeping_one_stale_stage_all_succeed() {
    let s = Scratch::new("sweep-race");
    let src = fixture(&s);
    for round in 0..5 {
        let stale = stale_stage(&s);
        let barrier = Arc::new(Barrier::new(3));
        let handles: Vec<_> = (0..3)
            .map(|i| {
                let (src, dst, barrier) = (
                    src.clone(),
                    s.path(&format!("dst{round}-{i}")),
                    barrier.clone(),
                );
                std::thread::spawn(move || {
                    barrier.wait();
                    clone_tree(&src, &dst, &opts()).map(|_| ())
                })
            })
            .collect();
        for h in handles {
            h.join()
                .unwrap()
                .expect("a concurrent sweep must not fail the clone");
        }
        assert!(!stale.exists());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_stale_stage_that_cannot_be_removed_does_not_block_clones() {
    let s = Scratch::new("sweep-stuck");
    let src = fixture(&s);
    let stale = stale_stage(&s);
    let stuck = stale.join("tree/d0/f0");
    let chflags = |flag: &str| {
        assert!(
            std::process::Command::new("chflags")
                .arg(flag)
                .arg(&stuck)
                .status()
                .unwrap()
                .success()
        )
    };
    chflags("uchg");
    let receipt = clone_tree(&src, &s.path("dst"), &opts());
    chflags("nouchg");
    assert!(receipt.unwrap().swept_stages.is_empty());
    assert_eq!(listing(&src), listing(&s.path("dst")));
    assert_eq!(
        sweep_stale_stages(&s.0).unwrap(),
        vec![stale],
        "retried once removable"
    );
}

#[test]
fn a_stage_is_ignored_by_git_in_an_enclosing_worktree() {
    let s = Scratch::new("git");
    let src = fixture(&s);
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&s.0)
            .args(args)
            .output()
    };
    match git(&["init", "-q"]) {
        Ok(o) if o.status.success() => {}
        _ => return, // no usable git here
    }
    let untracked = std::sync::Mutex::new(None);
    let hook = |_: u64| {
        let mut seen = untracked.lock().unwrap();
        if seen.is_none() {
            let out = git(&["status", "--porcelain", "--untracked-files=all"]).unwrap();
            *seen = Some(String::from_utf8_lossy(&out.stdout).into_owned());
        }
    };
    clone_tree(
        &src,
        &s.path("dst"),
        &Options {
            on_entry: Some(&hook),
            ..opts()
        },
    )
    .unwrap();
    let seen = untracked.into_inner().unwrap().unwrap();
    assert!(
        !seen.contains(clonedir::STAGE_PREFIX),
        "stage visible to git status: {seen}"
    );
}

// ------------------------------------------------------ physical allocation

#[cfg(target_os = "macos")]
mod physical {
    use super::*;

    const MIB: usize = 1 << 20;

    fn big_fixture(s: &Scratch) -> std::path::PathBuf {
        let src = s.path("src");
        write(&src.join("one"), &noise(2 * MIB, 1));
        write(&src.join("dir/two"), &noise(2 * MIB, 2));
        src
    }

    #[test]
    fn clones_share_extents_while_du_counts_them_twice() {
        let s = Scratch::new("share");
        let src = big_fixture(&s);
        let receipt = clone_tree(&src, &s.path("dst"), &opts()).unwrap();
        assert_eq!(
            (
                receipt.cloned_files,
                receipt.copied_bytes,
                receipt.logical_bytes
            ),
            (2, 0, 4 * MIB as u64)
        );

        for rel in ["one", "dir/two"] {
            let (a, b) = (
                sharing(&src.join(rel)).unwrap().unwrap(),
                sharing(&s.path("dst").join(rel)).unwrap().unwrap(),
            );
            assert_eq!(a.clone_id, b.clone_id, "{rel}: still one full clone group");
            assert!(a.clone_refcount.unwrap_or(0) >= 2, "{rel}: {a:?}");
            assert_eq!(
                (a.private_bytes, b.private_bytes),
                (0, 0),
                "{rel}: nothing private on either side"
            );
        }
        let (u_src, u_dst) = (measure(&src).unwrap(), measure(&s.path("dst")).unwrap());
        // du sees two full trees...
        assert!(u_src.allocated_bytes >= 4 * MIB as u64 && u_dst.allocated_bytes >= 4 * MIB as u64);
        // ...but neither holds a private byte: deleting either frees ~nothing.
        assert_eq!(
            (u_src.private_bytes, u_dst.private_bytes),
            (Some(0), Some(0))
        );
    }

    #[test]
    fn divergence_costs_only_the_blocks_written() {
        let s = Scratch::new("diverge");
        let src = big_fixture(&s);
        clone_tree(&src, &s.path("dst"), &opts()).unwrap();
        let target = s.path("dst/one");
        let mut f = fs::OpenOptions::new().write(true).open(&target).unwrap();
        f.seek(SeekFrom::Start(MIB as u64)).unwrap();
        f.write_all(&[0xAA; 4096]).unwrap();
        f.sync_all().unwrap();
        drop(f);

        let private = sharing(&target).unwrap().unwrap().private_bytes;
        assert!(
            (4096..=64 * 1024).contains(&private),
            "one rewritten page, not a whole copy: {private}"
        );
        assert_eq!(
            fs::read(src.join("one")).unwrap(),
            noise(2 * MIB, 1),
            "source bytes unchanged"
        );
        assert_eq!(
            sharing(&s.path("dst/dir/two"))
                .unwrap()
                .unwrap()
                .private_bytes,
            0,
            "untouched file still shared"
        );
    }

    #[test]
    fn deleting_the_source_moves_the_blocks_to_the_clone_instead_of_freeing_them() {
        let s = Scratch::new("retain");
        let src = big_fixture(&s);
        clone_tree(&src, &s.path("dst"), &opts()).unwrap();
        assert_eq!(measure(&s.path("dst")).unwrap().private_bytes, Some(0));
        force_remove(&src);
        let after = measure(&s.path("dst")).unwrap();
        // The clone now owns every block: the source's deletion reclaimed
        // roughly nothing, and only deleting the clone would.
        assert!(after.private_bytes.unwrap() >= 4 * MIB as u64, "{after:?}");
    }

    #[test]
    fn free_space_barely_moves_when_cloning() {
        // Other writers share this volume, so a single reading can be
        // disturbed; the clone has to show near-zero cost in one of three runs.
        let s = Scratch::new("statfs");
        write(&s.path("src/blob"), &noise(16 * MIB, 3));
        let mut observed = Vec::new();
        for attempt in 0..3 {
            let dst = s.path(&format!("dst{attempt}"));
            let r = clone_tree(&s.path("src"), &dst, &opts()).unwrap();
            let used = r.available_before as i64 - r.available_after as i64;
            observed.push(used);
            if used < (4 * MIB) as i64 {
                return;
            }
        }
        panic!("cloning 16 MiB consumed {observed:?} bytes of free space in every attempt");
    }
}
