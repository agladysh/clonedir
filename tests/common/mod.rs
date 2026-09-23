#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SERIAL: AtomicU64 = AtomicU64::new(0);

/// A fresh scratch directory under TMPDIR, removed on drop even if read-only
/// directories were left inside it.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        let n = SERIAL.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("clonedir-it-{name}-{}-{n}", std::process::id()));
        force_remove(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(fs::canonicalize(dir).unwrap())
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    /// Names in the scratch root, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        force_remove(&self.0);
    }
}

pub fn force_remove(path: &Path) {
    if let Ok(m) = fs::symlink_metadata(path) {
        if m.is_dir() {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
            if let Ok(entries) = fs::read_dir(path) {
                for e in entries.flatten() {
                    force_remove(&e.path());
                }
            }
            let _ = fs::remove_dir(path);
        } else {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
            let _ = fs::remove_file(path);
        }
    }
}

pub fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

/// Deterministic incompressible-looking bytes (xorshift), so fixtures need no RNG.
pub fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.extend_from_slice(&x.to_ne_bytes());
    }
    out.truncate(len);
    out
}

pub fn append(path: &Path, bytes: &[u8]) {
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

/// A relative listing of a tree: path, kind, and content or link target.
pub fn listing(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            let rel = p.strip_prefix(root).unwrap().display().to_string();
            let m = fs::symlink_metadata(&p).unwrap();
            if m.file_type().is_symlink() {
                out.push(format!("{rel} -> {}", fs::read_link(&p).unwrap().display()));
            } else if m.is_dir() {
                out.push(format!("{rel}/"));
                walk(root, &p, out);
            } else {
                let data = fs::read(&p).unwrap();
                out.push(format!(
                    "{rel} [{} bytes, sum {}]",
                    data.len(),
                    data.iter().map(|&b| b as u64).sum::<u64>()
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

unsafe extern "C" {
    fn mkfifo(path: *const std::ffi::c_char, mode: u32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn getuid() -> u32;
}

pub fn make_fifo(path: &Path) {
    let c = std::ffi::CString::new(path.as_os_str().to_str().unwrap()).unwrap();
    assert_eq!(unsafe { mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo");
}

pub fn signal(pid: u32, sig: i32) {
    assert_eq!(unsafe { kill(pid as i32, sig) }, 0, "kill");
}

pub fn is_root() -> bool {
    unsafe { getuid() == 0 }
}

/// A pid that certainly belonged to a process that has exited.
pub fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

/// Stage directories clonedir left in `parent`.
pub fn stages(parent: &Path) -> Vec<PathBuf> {
    fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(clonedir::STAGE_PREFIX)
        })
        .collect()
}
