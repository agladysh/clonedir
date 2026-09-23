//! Copy-on-write directory cloning with explicit semantics.
//!
//! [`clone_tree`] materializes a fresh, independent copy of a directory tree
//! whose regular files share extents with the source (APFS `clonefile`, Linux
//! `FICLONE`). The contract, which the tests exercise:
//!
//! - **Structure.** Directories, regular files and symlinks are reproduced at
//!   their own paths. Symlinks are recreated verbatim and never followed; their
//!   own times are not kept. File modes and modification times are kept;
//!   directory modes and times are applied after their contents. Ownership is
//!   not copied. On macOS `clonefile` also carries a file's extended
//!   attributes, ACL and flags (including `uchg`); the Linux path, byte copies
//!   and directories carry mode and times only. FIFOs, sockets and devices,
//!   and entries on another device (mount points, and on Linux btrfs
//!   subvolumes), are refused. Bind mounts of the same filesystem are not
//!   detected.
//! - **Independence.** Every destination file is its own inode. Writes on
//!   either side never reach the other; source hard links become separate
//!   files in the destination. A symlink is copied verbatim, so an absolute
//!   link into the source still points into the source.
//! - **No silent byte copies.** When the two sides cannot share extents
//!   (another volume, a filesystem without cloning) the operation fails
//!   unless [`Fallback::Copy`] was asked for, and the receipt counts every
//!   byte copied.
//! - **Checked space, not reserved.** It refuses to start below
//!   `min_free_bytes`, rechecks every few hundred entries, and before each
//!   byte copy requires room for that file above the floor. Other writers on
//!   the volume, including concurrent clones, can still take the space between
//!   a check and the write; an `ENOSPC` then fails the clone and frees its stage.
//!   Nothing bounds later growth when either side is written.
//! - **Atomic publication.** Work happens in a private stage beside the
//!   destination, which appears only complete, by an exclusive rename. An
//!   existing destination is never replaced or merged into. Publication is not
//!   made durable with `fsync`.
//! - **Consistent or refused.** Every source entry's inode, size, mode, mtime
//!   and ctime are recorded before it is copied and re-examined after the
//!   whole walk; any difference discards the copy with
//!   [`ErrorKind::SourceChanged`]. When the check passes, each entry was
//!   unchanged from before it was copied until after the last one was, so the
//!   copy is the tree as it stood at one instant, provided every change moved
//!   that metadata. `write(2)`, `rename(2)` and `chmod(2)` do; writes through
//!   an already-dirty shared memory map may not, and same-size rewrites within
//!   one timestamp tick are invisible on filesystems with coarse timestamps.
//!   It is a filesystem-level instant, not an application-level one: a writer
//!   that pauses between two related writes for the whole walk yields a
//!   successful copy of its intermediate state.
//! - **Own cleanup.** On failure or cancellation the stage is removed. A stage
//!   left by a killed process names its owner pid, and [`sweep_stale_stages`]
//!   (run automatically before each clone into the same parent) removes it once
//!   that process is gone. A stage kept with `keep_failed_stage` is never
//!   swept. Stages are ignored by Git. Nobody writes into a stage but
//!   clonedir, and nothing was published from it, but it can hold the only
//!   remaining copy of source contents that changed or were deleted since;
//!   clonedir treats that as its own disposable output. Pids are only
//!   meaningful on one host, so a parent shared between hosts or pid
//!   namespaces must not hold concurrent clones.
//!
//! What cloning does *not* do: extents are shared only between the files it
//! clones. It does not deduplicate independently written data (for example
//! separately compressed Git objects), impose retention, or make deleting the
//! source reclaim space while a clone still holds its blocks. [`measure`]
//! reports logical, allocated (`du`) and private bytes so callers can tell
//! those apart.

mod sys;

pub use sys::{Filesystem, INTERRUPTED, Sharing, install_interrupt_handler};

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File, FileTimes, Metadata};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Name prefix of the private stage directories clonedir creates.
pub const STAGE_PREFIX: &str = ".clonedir-stage.";
const OWNER_FILE: &str = "owner";
const OWNER_MAGIC: &str = "clonedir-stage v1";
/// Present in a stage kept on request; the sweep leaves such a stage alone.
const KEPT_FILE: &str = "kept";
const SPACE_CHECK_EVERY: u64 = 256;

/// What to do when source and destination cannot share extents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    /// Fail with [`ErrorKind::CrossFilesystem`] or [`ErrorKind::NotClonable`].
    Fail,
    /// Copy bytes, within the space bound, and count them in the receipt.
    Copy,
}

pub struct Options<'a> {
    pub fallback: Fallback,
    /// Available space that must remain on the destination filesystem.
    pub min_free_bytes: u64,
    /// Abandon the operation, removing the stage, once this reads true.
    pub cancel: Option<&'a AtomicBool>,
    /// Called with the running entry count after each entry is materialized.
    pub on_entry: Option<&'a (dyn Fn(u64) + Sync)>,
    /// Leave a failed stage in place (for diagnosis) instead of removing it.
    pub keep_failed_stage: bool,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options {
            fallback: Fallback::Fail,
            min_free_bytes: 256 << 20,
            cancel: None,
            on_entry: None,
            keep_failed_stage: false,
        }
    }
}

/// What a successful clone did.
#[derive(Debug, Clone, Default)]
pub struct Receipt {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub filesystem: String,
    pub directories: u64,
    pub regular_files: u64,
    pub symlinks: u64,
    /// Regular files materialized by sharing extents.
    pub cloned_files: u64,
    /// Regular files materialized by copying bytes ([`Fallback::Copy`] only).
    pub copied_files: u64,
    /// Sum of regular file lengths; what `du` would roughly report for the copy.
    pub logical_bytes: u64,
    /// Bytes actually written as new data. Zero when everything was cloned.
    pub copied_bytes: u64,
    /// Source files with more than one hard link; each is now an independent file.
    pub hardlinked_files: u64,
    /// Stale stages of dead clonedir processes removed before starting.
    pub swept_stages: Vec<PathBuf>,
    pub available_before: u64,
    pub available_after: u64,
}

/// The resolved inputs of a clone, as checked before anything is created.
#[derive(Debug, Clone)]
pub struct Plan {
    pub source: PathBuf,
    pub destination: PathBuf,
    /// Source and destination parent are on different filesystems.
    pub cross_filesystem: bool,
    pub filesystem: Filesystem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidSource,
    InvalidDestination,
    DestinationExists,
    /// The destination would lie inside the source.
    Nested,
    PermissionDenied,
    /// Source and destination are on different filesystems and `Fallback::Fail`.
    CrossFilesystem,
    /// A file could not be cloned on this filesystem and `Fallback::Fail`.
    NotClonable,
    /// A mount point inside the source.
    MountPointInside,
    /// A FIFO, socket or device inside the source.
    SpecialFile,
    InsufficientSpace,
    /// The source changed while it was being copied; the copy was discarded.
    SourceChanged,
    Interrupted,
    Io,
}

#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub path: Option<PathBuf>,
    pub detail: String,
    pub io: Option<io::Error>,
    /// A failed stage kept because `keep_failed_stage` was set.
    pub kept_stage: Option<PathBuf>,
}

impl Error {
    fn new(kind: ErrorKind, path: Option<&Path>, detail: impl Into<String>) -> Self {
        Error {
            kind,
            path: path.map(Path::to_path_buf),
            detail: detail.into(),
            io: None,
            kept_stage: None,
        }
    }

    fn io(path: &Path, error: io::Error) -> Self {
        let kind = match error.kind() {
            io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
            _ => ErrorKind::Io,
        };
        Error {
            kind,
            path: Some(path.to_path_buf()),
            detail: error.to_string(),
            io: Some(error),
            kept_stage: None,
        }
    }

    /// Process exit status for this failure; distinct where a caller can act on it.
    pub fn exit_code(&self) -> i32 {
        match self.kind {
            ErrorKind::SourceChanged => 3,
            ErrorKind::CrossFilesystem | ErrorKind::NotClonable => 4,
            ErrorKind::InsufficientSpace => 5,
            ErrorKind::DestinationExists => 6,
            ErrorKind::Interrupted => 130,
            _ => 1,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.detail)?;
        if let Some(path) = &self.path {
            write!(f, ": {}", path.display())?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

// ---------------------------------------------------------------- preflight

/// Resolve and check the inputs without creating anything.
pub fn plan(source: &Path, destination: &Path, options: &Options) -> Result<Plan> {
    let source = fs::canonicalize(source).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::new(
            ErrorKind::InvalidSource,
            Some(source),
            "source directory does not exist",
        ),
        _ => Error::io(source, e),
    })?;
    let source_meta = fs::symlink_metadata(&source).map_err(|e| Error::io(&source, e))?;
    if !source_meta.is_dir() {
        return Err(Error::new(
            ErrorKind::InvalidSource,
            Some(&source),
            "source is not a directory",
        ));
    }

    let name = match destination.file_name() {
        Some(name) if name != OsStr::new(".") && name != OsStr::new("..") => name.to_os_string(),
        _ => {
            return Err(Error::new(
                ErrorKind::InvalidDestination,
                Some(destination),
                "destination must name a new entry",
            ));
        }
    };
    let parent = match destination.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let parent = fs::canonicalize(parent).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::new(
            ErrorKind::InvalidDestination,
            Some(parent),
            "parent directory of destination does not exist",
        ),
        _ => Error::io(parent, e),
    })?;
    let parent_meta = fs::metadata(&parent).map_err(|e| Error::io(&parent, e))?;
    if !parent_meta.is_dir() {
        return Err(Error::new(
            ErrorKind::InvalidDestination,
            Some(&parent),
            "parent of destination is not a directory",
        ));
    }
    let destination = parent.join(&name);
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(Error::new(
            ErrorKind::DestinationExists,
            Some(&destination),
            "destination exists; clonedir never overwrites or merges into it",
        ));
    }
    // A path prefix misses aliases such as APFS firmlinks
    // (/System/Volumes/Data/...), so compare identities as well.
    let source_id = (source_meta.dev(), source_meta.ino());
    let nested = destination.starts_with(&source)
        || parent
            .ancestors()
            .any(|a| fs::metadata(a).is_ok_and(|m| (m.dev(), m.ino()) == source_id));
    if nested {
        return Err(Error::new(
            ErrorKind::Nested,
            Some(&destination),
            "destination lies inside the source",
        ));
    }
    let cross_filesystem = source_meta.dev() != parent_meta.dev();
    if cross_filesystem && options.fallback == Fallback::Fail {
        return Err(Error::new(
            ErrorKind::CrossFilesystem,
            Some(&destination),
            "source and destination are on different filesystems, so nothing can be cloned; \
             pass the copy fallback to copy bytes",
        ));
    }
    let filesystem = sys::filesystem(&parent).map_err(|e| Error::io(&parent, e))?;
    if filesystem.available_bytes < options.min_free_bytes {
        return Err(Error::new(
            ErrorKind::InsufficientSpace,
            Some(&parent),
            format!(
                "{} bytes available, below the {} byte floor",
                filesystem.available_bytes, options.min_free_bytes
            ),
        ));
    }
    Ok(Plan {
        source,
        destination,
        cross_filesystem,
        filesystem,
    })
}

// -------------------------------------------------------------------- clone

/// Clone `source` to the new path `destination`. See the crate documentation.
pub fn clone_tree(source: &Path, destination: &Path, options: &Options) -> Result<Receipt> {
    let plan = plan(source, destination, options)?;
    let parent = plan
        .destination
        .parent()
        .expect("resolved destination has a parent")
        .to_path_buf();
    let name = plan
        .destination
        .file_name()
        .expect("resolved destination has a name")
        .to_os_string();
    // Housekeeping only: a stage that cannot be removed must not stop this clone.
    let swept = sweep_stale_stages(&parent).unwrap_or_default();

    let stage = create_stage(&parent, &name)?;
    let tree = stage.join("tree");
    let mut walk = Walk {
        options,
        root_dev: fs::symlink_metadata(&plan.source)
            .map_err(|e| Error::io(&plan.source, e))?
            .dev(),
        space_path: parent.clone(),
        records: Vec::new(),
        hardlinks: HashSet::new(),
        entries: 0,
        receipt: Receipt {
            source: plan.source.clone(),
            destination: plan.destination.clone(),
            filesystem: plan.filesystem.type_name.clone(),
            swept_stages: swept,
            available_before: plan.filesystem.available_bytes,
            ..Receipt::default()
        },
    };

    let outcome = (|| {
        let root_meta =
            fs::symlink_metadata(&plan.source).map_err(|e| Error::io(&plan.source, e))?;
        walk.directory(&plan.source, &tree, &root_meta)?;
        walk.verify_unchanged()?;
        // Moving a directory to another parent needs write permission on it
        // (for its ".."), so a read-only root gets its own mode once published.
        let root_mode = root_meta.mode() & 0o7777;
        if root_mode & 0o700 != 0o700 {
            fs::set_permissions(&tree, fs::Permissions::from_mode(root_mode | 0o700))
                .map_err(|e| Error::io(&tree, e))?;
        }
        match sys::rename_noreplace(&tree, &plan.destination) {
            Ok(()) => fs::set_permissions(&plan.destination, fs::Permissions::from_mode(root_mode))
                .map_err(|e| Error::io(&plan.destination, e)),
            Err(e)
                if e.kind() == io::ErrorKind::AlreadyExists
                    || e.raw_os_error() == Some(66 /* ENOTEMPTY */) =>
            {
                Err(Error::new(
                    ErrorKind::DestinationExists,
                    Some(&plan.destination),
                    "destination appeared while cloning; it was not touched",
                ))
            }
            Err(e) => Err(Error::io(&plan.destination, e)),
        }
    })();

    match outcome {
        Ok(()) => {
            remove_tree(&stage).map_err(|e| Error::io(&stage, e))?;
            let mut receipt = walk.receipt;
            receipt.available_after = sys::filesystem(&parent)
                .map(|f| f.available_bytes)
                .unwrap_or(0);
            Ok(receipt)
        }
        Err(mut error) => {
            if options.keep_failed_stage {
                // Kept on request, so no later sweep may take it.
                let _ = fs::File::create(stage.join(KEPT_FILE));
                error.kept_stage = Some(stage);
            } else if let Err(e) = remove_tree(&stage) {
                error.detail =
                    format!("{}; and the stage could not be removed ({e})", error.detail);
                error.kept_stage = Some(stage);
            }
            Err(error)
        }
    }
}

struct Record {
    path: PathBuf,
    ino: u64,
    size: u64,
    mode: u32,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

impl Record {
    fn of(path: &Path, m: &Metadata) -> Self {
        Record {
            path: path.to_path_buf(),
            ino: m.ino(),
            size: m.size(),
            mode: m.mode(),
            mtime: (m.mtime(), m.mtime_nsec()),
            ctime: (m.ctime(), m.ctime_nsec()),
        }
    }

    fn matches(&self, m: &Metadata) -> bool {
        self.ino == m.ino()
            && self.size == m.size()
            && self.mode == m.mode()
            && self.mtime == (m.mtime(), m.mtime_nsec())
            && self.ctime == (m.ctime(), m.ctime_nsec())
    }
}

struct Walk<'a> {
    options: &'a Options<'a>,
    root_dev: u64,
    space_path: PathBuf,
    records: Vec<Record>,
    hardlinks: HashSet<(u64, u64)>,
    entries: u64,
    receipt: Receipt,
}

impl Walk<'_> {
    fn tick(&mut self) -> Result<()> {
        if self
            .options
            .cancel
            .is_some_and(|c| c.load(Ordering::SeqCst))
        {
            return Err(Error::new(
                ErrorKind::Interrupted,
                None,
                "interrupted; the partial copy was discarded",
            ));
        }
        self.entries += 1;
        if self.entries.is_multiple_of(SPACE_CHECK_EVERY) {
            self.ensure_space(0)?;
        }
        Ok(())
    }

    fn ensure_space(&self, extra: u64) -> Result<()> {
        let available = sys::filesystem(&self.space_path)
            .map_err(|e| Error::io(&self.space_path, e))?
            .available_bytes;
        if available < self.options.min_free_bytes.saturating_add(extra) {
            return Err(Error::new(
                ErrorKind::InsufficientSpace,
                Some(&self.space_path),
                format!(
                    "{available} bytes available; continuing would pass the {} byte floor",
                    self.options.min_free_bytes
                ),
            ));
        }
        Ok(())
    }

    fn after_entry(&self) {
        if let Some(hook) = self.options.on_entry {
            hook(self.entries);
        }
    }

    fn directory(&mut self, src: &Path, dst: &Path, meta: &Metadata) -> Result<()> {
        self.records.push(Record::of(src, meta));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(dst)
            .map_err(|e| Error::io(dst, e))?;
        let mut names: Vec<OsString> = fs::read_dir(src)
            .map_err(|e| Error::io(src, e))?
            .map(|entry| entry.map(|e| e.file_name()))
            .collect::<io::Result<_>>()
            .map_err(|e| Error::io(src, e))?;
        names.sort();
        for name in names {
            self.tick()?;
            let (s, d) = (src.join(&name), dst.join(&name));
            let m = match fs::symlink_metadata(&s) {
                Ok(m) => m,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    return Err(Error::new(
                        ErrorKind::SourceChanged,
                        Some(&s),
                        "source entry vanished while cloning",
                    ));
                }
                Err(e) => return Err(Error::io(&s, e)),
            };
            if m.dev() != self.root_dev {
                return Err(Error::new(
                    ErrorKind::MountPointInside,
                    Some(&s),
                    "source contains a mount point",
                ));
            }
            let kind = m.file_type();
            if kind.is_symlink() {
                self.records.push(Record::of(&s, &m));
                let target = fs::read_link(&s).map_err(|e| Error::io(&s, e))?;
                std::os::unix::fs::symlink(&target, &d).map_err(|e| Error::io(&d, e))?;
                self.receipt.symlinks += 1;
            } else if kind.is_file() {
                self.file(&s, &d, &m)?;
            } else if kind.is_dir() {
                self.directory(&s, &d, &m)?;
            } else {
                return Err(Error::new(
                    ErrorKind::SpecialFile,
                    Some(&s),
                    "source contains a FIFO, socket or device",
                ));
            }
            self.after_entry();
        }
        // Contents first, then the directory's own times and mode, so a
        // read-only source directory can still be populated.
        let times = FileTimes::new()
            .set_accessed(meta.accessed().map_err(|e| Error::io(src, e))?)
            .set_modified(meta.modified().map_err(|e| Error::io(src, e))?);
        File::open(dst)
            .and_then(|f| f.set_times(times))
            .map_err(|e| Error::io(dst, e))?;
        fs::set_permissions(dst, fs::Permissions::from_mode(meta.mode() & 0o7777))
            .map_err(|e| Error::io(dst, e))?;
        self.receipt.directories += 1;
        Ok(())
    }

    fn file(&mut self, src: &Path, dst: &Path, meta: &Metadata) -> Result<()> {
        self.records.push(Record::of(src, meta));
        if meta.nlink() > 1 && self.hardlinks.insert((meta.dev(), meta.ino())) {
            self.receipt.hardlinked_files += 1;
        }
        match clone_file(src, dst) {
            Ok(()) => self.receipt.cloned_files += 1,
            Err(e) if sys::is_unclonable(&e) => match self.options.fallback {
                Fallback::Fail => {
                    return Err(Error::new(
                        ErrorKind::NotClonable,
                        Some(src),
                        format!(
                            "cannot clone on this filesystem ({e}); pass the copy fallback to copy bytes"
                        ),
                    ));
                }
                Fallback::Copy => {
                    self.ensure_space(meta.size())?;
                    copy_file(src, dst, meta).map_err(|e| Error::io(dst, e))?;
                    self.receipt.copied_files += 1;
                    self.receipt.copied_bytes += meta.size();
                }
            },
            Err(e) => return Err(Error::io(src, e)),
        }
        self.receipt.regular_files += 1;
        self.receipt.logical_bytes += meta.size();
        Ok(())
    }

    /// Every source entry must still be the one that was copied.
    fn verify_unchanged(&self) -> Result<()> {
        let changed: Vec<&Path> = self
            .records
            .iter()
            .filter(|r| !fs::symlink_metadata(&r.path).is_ok_and(|m| r.matches(&m)))
            .map(|r| r.path.as_path())
            .take(10)
            .collect();
        if let Some(first) = changed.first() {
            let list: Vec<String> = changed.iter().map(|p| p.display().to_string()).collect();
            return Err(Error::new(
                ErrorKind::SourceChanged,
                Some(first),
                format!(
                    "the source changed while it was being cloned, so the copy was discarded \
                     (stop its writers and retry); changed: {}",
                    list.join(", ")
                ),
            ));
        }
        Ok(())
    }
}

#[cfg(not(test))]
fn clone_file(src: &Path, dst: &Path) -> io::Result<()> {
    sys::clone_file(src, dst)
}

// Unit tests can make cloning report "unsupported" to exercise the fallback.
#[cfg(test)]
thread_local!(static FORCE_UNCLONABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

#[cfg(test)]
fn clone_file(src: &Path, dst: &Path) -> io::Result<()> {
    if FORCE_UNCLONABLE.with(|f| f.get()) {
        return Err(io::Error::from_raw_os_error(18 /* EXDEV */));
    }
    sys::clone_file(src, dst)
}

fn copy_file(src: &Path, dst: &Path, meta: &Metadata) -> io::Result<()> {
    let mut from = File::open(src)?;
    let mut to = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)?;
    io::copy(&mut from, &mut to)?;
    to.flush()?;
    to.set_times(
        FileTimes::new()
            .set_accessed(meta.accessed()?)
            .set_modified(meta.modified()?),
    )?;
    fs::set_permissions(dst, fs::Permissions::from_mode(meta.mode() & 0o7777))
}

// ------------------------------------------------------ stages and cleanup

static STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn create_stage(parent: &Path, name: &OsStr) -> Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let pid = sys::current_pid();
    let serial = STAGE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let stage = parent.join(format!(
        "{STAGE_PREFIX}{pid}.{:x}{:x}",
        nanos.as_nanos(),
        serial
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&stage)
        .map_err(|e| Error::io(&stage, e))?;
    let owner = format!(
        "{OWNER_MAGIC}\npid={pid}\nstarted={}\ndestination={}\n",
        nanos.as_secs(),
        name.to_string_lossy()
    );
    // The .gitignore keeps the stage out of `git status` and `git add` when the
    // parent is inside a worktree: its paths do not match the ignore rules of
    // the destination (for example `/node_modules/`), and a killed run's stage
    // stays until the next clone into this parent.
    let write = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(stage.join(OWNER_FILE))
        .and_then(|mut f| f.write_all(owner.as_bytes()))
        .and_then(|()| fs::write(stage.join(".gitignore"), "*\n"));
    if let Err(e) = write {
        let _ = remove_tree(&stage);
        return Err(Error::io(&stage, e));
    }
    Ok(stage)
}

fn stage_owner(stage: &Path) -> Option<u32> {
    let text = fs::read_to_string(stage.join(OWNER_FILE)).ok()?;
    let mut lines = text.lines();
    if lines.next()? != OWNER_MAGIC {
        return None;
    }
    lines
        .find_map(|l| l.strip_prefix("pid="))
        .and_then(|p| p.parse().ok())
}

/// Remove stages in `parent` left by clonedir processes that no longer run.
///
/// Only directories named with [`STAGE_PREFIX`], owned by the current user and
/// holding a clonedir owner record whose pid is not alive are removed. A stage
/// without a readable owner record, whose pid is alive (even if reused), or
/// kept on request (`keep_failed_stage`) is left alone, and so is one that
/// cannot be removed (it is retried next time). Concurrent sweeps of one parent
/// are safe.
pub fn sweep_stale_stages(parent: &Path) -> io::Result<Vec<PathBuf>> {
    let me = sys::current_uid();
    let mut removed = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGE_PREFIX)
        {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_dir() || meta.uid() != me {
            continue;
        }
        if path.join(KEPT_FILE).exists() {
            continue;
        }
        if stage_owner(&path).is_some_and(|pid| !sys::process_alive(pid))
            && remove_tree(&path).is_ok()
        {
            removed.push(path);
        }
    }
    Ok(removed)
}

/// Remove a tree without following links, restoring owner access to
/// directories first so read-only copies (for example module caches) go too.
/// Entries that vanish meanwhile (another sweep of the same stage) count as
/// removed.
fn remove_tree(path: &Path) -> io::Result<()> {
    fn gone<T>(result: io::Result<T>) -> io::Result<Option<T>> {
        match result {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
    let Some(meta) = gone(fs::symlink_metadata(path))? else {
        return Ok(());
    };
    if !meta.is_dir() {
        return gone(fs::remove_file(path)).map(drop);
    }
    if meta.mode() & 0o700 != 0o700 {
        gone(fs::set_permissions(
            path,
            fs::Permissions::from_mode((meta.mode() & 0o7777) | 0o700),
        ))?;
    }
    let Some(entries) = gone(fs::read_dir(path))? else {
        return Ok(());
    };
    for entry in entries {
        if let Some(entry) = gone(entry)? {
            remove_tree(&entry.path())?;
        }
    }
    gone(fs::remove_dir(path)).map(drop)
}

// ------------------------------------------------------------- measurement

/// Space accounting for a tree, separating what `du` counts from what
/// removing the tree would free.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Regular files, each hard-linked inode counted once.
    pub files: u64,
    /// Sum of file lengths.
    pub logical_bytes: u64,
    /// Allocated blocks of files and directories, as `du` reports them.
    /// Shared clone extents are counted once per file that shares them.
    pub allocated_bytes: u64,
    /// Bytes of file data not shared with any other file (APFS), i.e. roughly
    /// what deleting the files would free. `None` where the platform does not
    /// report it. Snapshots can still hold blocks after deletion.
    pub private_bytes: Option<u64>,
}

/// Measure a tree without following links.
pub fn measure(path: &Path) -> io::Result<Usage> {
    fn walk(path: &Path, seen: &mut HashSet<(u64, u64)>, usage: &mut Usage) -> io::Result<()> {
        let meta = fs::symlink_metadata(path)?;
        if !seen.insert((meta.dev(), meta.ino())) {
            return Ok(());
        }
        usage.allocated_bytes += meta.blocks() * 512;
        if meta.is_dir() {
            for entry in fs::read_dir(path)? {
                walk(&entry?.path(), seen, usage)?;
            }
        } else if meta.is_file() {
            usage.files += 1;
            usage.logical_bytes += meta.size();
            match (usage.private_bytes, sys::sharing(path)?) {
                (Some(total), Some(s)) => usage.private_bytes = Some(total + s.private_bytes),
                _ => usage.private_bytes = None,
            }
        }
        Ok(())
    }
    let mut usage = Usage {
        private_bytes: Some(0),
        ..Usage::default()
    };
    walk(path, &mut HashSet::new(), &mut usage)?;
    Ok(usage)
}

/// Extent-sharing attributes of one file, where the platform reports them.
pub fn sharing(path: &Path) -> io::Result<Option<Sharing>> {
    sys::sharing(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clonedir-unit-{name}-{}", sys::current_pid()));
        let _ = remove_tree(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn options() -> Options<'static> {
        Options {
            min_free_bytes: 0,
            ..Options::default()
        }
    }

    #[test]
    fn unclonable_files_fail_by_default_and_leave_nothing() {
        let root = scratch("fallback-fail");
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/a"), b"abc").unwrap();
        FORCE_UNCLONABLE.with(|f| f.set(true));
        let error = clone_tree(&root.join("src"), &root.join("dst"), &options()).unwrap_err();
        FORCE_UNCLONABLE.with(|f| f.set(false));
        assert_eq!(error.kind, ErrorKind::NotClonable);
        assert_eq!(error.exit_code(), 4);
        assert!(!root.join("dst").exists());
        assert_eq!(
            fs::read_dir(&root).unwrap().count(),
            1,
            "no stage left behind"
        );
        remove_tree(&root).unwrap();
    }

    #[test]
    fn copy_fallback_counts_bytes_and_keeps_metadata() {
        let root = scratch("fallback-copy");
        fs::create_dir_all(root.join("src/d")).unwrap();
        fs::write(root.join("src/d/a"), vec![7u8; 10_000]).unwrap();
        fs::set_permissions(root.join("src/d/a"), fs::Permissions::from_mode(0o640)).unwrap();
        FORCE_UNCLONABLE.with(|f| f.set(true));
        let receipt = clone_tree(
            &root.join("src"),
            &root.join("dst"),
            &Options {
                fallback: Fallback::Copy,
                ..options()
            },
        )
        .unwrap();
        FORCE_UNCLONABLE.with(|f| f.set(false));
        assert_eq!(
            (
                receipt.cloned_files,
                receipt.copied_files,
                receipt.copied_bytes
            ),
            (0, 1, 10_000)
        );
        let (s, d) = (
            fs::metadata(root.join("src/d/a")).unwrap(),
            fs::metadata(root.join("dst/d/a")).unwrap(),
        );
        assert_eq!(fs::read(root.join("dst/d/a")).unwrap(), vec![7u8; 10_000]);
        assert_eq!(
            (d.mode() & 0o7777, d.mtime(), d.mtime_nsec()),
            (0o640, s.mtime(), s.mtime_nsec())
        );
        if let Some(sharing) = sharing(&root.join("dst/d/a")).unwrap() {
            assert!(
                sharing.private_bytes >= 10_000,
                "a byte copy owns its data: {sharing:?}"
            );
        }
        remove_tree(&root).unwrap();
    }

    #[test]
    fn copy_fallback_respects_the_space_floor() {
        let root = scratch("fallback-space");
        fs::create_dir(root.join("src")).unwrap();
        // Sparse, so the fixture itself allocates nothing.
        File::create(root.join("src/a"))
            .unwrap()
            .set_len(64 << 20)
            .unwrap();
        let available = sys::filesystem(&root).unwrap().available_bytes;
        // Room to start, but not for the 64 MiB the copy would write.
        let floor = available.saturating_sub(1 << 20);
        FORCE_UNCLONABLE.with(|f| f.set(true));
        let error = clone_tree(
            &root.join("src"),
            &root.join("dst"),
            &Options {
                fallback: Fallback::Copy,
                min_free_bytes: floor,
                ..Options::default()
            },
        )
        .unwrap_err();
        FORCE_UNCLONABLE.with(|f| f.set(false));
        assert_eq!(error.kind, ErrorKind::InsufficientSpace);
        assert!(!root.join("dst").exists());
        remove_tree(&root).unwrap();
    }
}
