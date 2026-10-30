//! The few platform calls clonedir needs, declared directly so the crate has no
//! dependencies. Everything here is a thin, checked wrapper; policy lives in
//! `lib.rs`.

use std::ffi::{CString, c_char, c_int, c_void};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub fn cstr(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))
}

fn check(ret: c_int) -> io::Result<()> {
    if ret == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

unsafe extern "C" {
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn getpid() -> c_int;
    fn getuid() -> u32;
}

pub fn current_uid() -> u32 {
    unsafe { getuid() }
}

pub fn current_pid() -> u32 {
    unsafe { getpid() as u32 }
}

/// Whether `pid` names a process that exists on this host. EPERM means it
/// exists but belongs to someone else, which still counts as alive.
pub fn process_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    let ret = unsafe { kill(pid as c_int, 0) };
    ret == 0 || io::Error::last_os_error().raw_os_error() != Some(3 /* ESRCH */)
}

/// Errors that mean "this filesystem pair cannot share extents", as opposed to
/// a failure that copying would not fix either.
pub fn is_unclonable(error: &io::Error) -> bool {
    #[cfg(target_os = "macos")]
    const CODES: &[i32] = &[
        18,  /* EXDEV */
        45,  /* ENOTSUP */
        102, /* EOPNOTSUPP */
    ];
    // FICLONE reports a filesystem without reflink as EOPNOTSUPP, EINVAL or ENOTTY.
    #[cfg(target_os = "linux")]
    const CODES: &[i32] = &[
        18, /* EXDEV */
        95, /* EOPNOTSUPP */
        22, /* EINVAL */
        25, /* ENOTTY */
    ];
    error
        .raw_os_error()
        .is_some_and(|code| CODES.contains(&code))
}

// ------------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
mod imp {
    use super::*;

    const CLONE_NOFOLLOW: u32 = 0x0001;
    const CLONE_NOOWNERCOPY: u32 = 0x0002;
    const RENAME_EXCL: u32 = 0x0000_0004;

    const ATTR_BIT_MAP_COUNT: u16 = 5;
    const ATTR_CMN_RETURNED_ATTRS: u32 = 0x8000_0000;
    const ATTR_CMNEXT_PRIVATESIZE: u32 = 0x0000_0008;
    const ATTR_CMNEXT_CLONEID: u32 = 0x0000_0100;
    const ATTR_CMNEXT_CLONE_REFCNT: u32 = 0x0000_1000;
    const FSOPT_NOFOLLOW: u32 = 0x0000_0001;
    const FSOPT_ATTR_CMN_EXTENDED: u32 = 0x0000_0020;

    #[repr(C)]
    struct AttrList {
        bitmapcount: u16,
        reserved: u16,
        commonattr: u32,
        volattr: u32,
        dirattr: u32,
        fileattr: u32,
        forkattr: u32,
    }

    unsafe extern "C" {
        fn clonefile(src: *const c_char, dst: *const c_char, flags: u32) -> c_int;
        fn renamex_np(from: *const c_char, to: *const c_char, flags: u32) -> c_int;
        fn getattrlist(
            path: *const c_char,
            list: *mut AttrList,
            buf: *mut c_void,
            size: usize,
            options: u32,
        ) -> c_int;
        #[cfg_attr(target_arch = "x86_64", link_name = "statfs$INODE64")]
        fn statfs(path: *const c_char, buf: *mut c_void) -> c_int;
    }

    /// clonefile(2) of one regular file or symlink, never following links and
    /// never copying ownership. Fails if `dst` exists.
    pub fn clone_file(src: &Path, dst: &Path) -> io::Result<()> {
        let (s, d) = (cstr(src)?, cstr(dst)?);
        check(unsafe { clonefile(s.as_ptr(), d.as_ptr(), CLONE_NOFOLLOW | CLONE_NOOWNERCOPY) })
    }

    /// rename(2) that fails with EEXIST instead of replacing `to`, including an
    /// empty directory at `to`, which plain rename would silently replace.
    pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
        let (f, t) = (cstr(from)?, cstr(to)?);
        check(unsafe { renamex_np(f.as_ptr(), t.as_ptr(), RENAME_EXCL) })
    }

    pub fn filesystem(path: &Path) -> io::Result<super::Filesystem> {
        let p = cstr(path)?;
        let mut buf = [0u8; 4096]; // struct statfs is 2168 bytes; spare room is harmless
        check(unsafe { statfs(p.as_ptr(), buf.as_mut_ptr().cast()) })?;
        let u32_at = |o: usize| u32::from_ne_bytes(buf[o..o + 4].try_into().unwrap()) as u64;
        let u64_at = |o: usize| u64::from_ne_bytes(buf[o..o + 8].try_into().unwrap());
        let name = &buf[72..88];
        let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        Ok(super::Filesystem {
            available_bytes: u32_at(0) * u64_at(24),
            type_name: String::from_utf8_lossy(&name[..len]).into_owned(),
        })
    }

    pub fn sharing(path: &Path) -> io::Result<Option<super::Sharing>> {
        let p = cstr(path)?;
        let mut list = AttrList {
            bitmapcount: ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: ATTR_CMN_RETURNED_ATTRS,
            volattr: 0,
            dirattr: 0,
            fileattr: 0,
            forkattr: ATTR_CMNEXT_PRIVATESIZE | ATTR_CMNEXT_CLONEID | ATTR_CMNEXT_CLONE_REFCNT,
        };
        let mut buf = [0u8; 256];
        check(unsafe {
            getattrlist(
                p.as_ptr(),
                &mut list,
                buf.as_mut_ptr().cast(),
                buf.len(),
                FSOPT_NOFOLLOW | FSOPT_ATTR_CMN_EXTENDED,
            )
        })?;
        // u32 length, then attribute_set_t (five u32 masks), then the values of
        // the returned bits in ascending bit order, each 4-byte aligned.
        let returned_fork = u32::from_ne_bytes(buf[20..24].try_into().unwrap());
        let mut at = 24usize;
        let mut take = |n: usize| {
            let v = &buf[at..at + n];
            at += n.next_multiple_of(4);
            v.to_vec()
        };
        let private = (returned_fork & ATTR_CMNEXT_PRIVATESIZE != 0)
            .then(|| i64::from_ne_bytes(take(8).try_into().unwrap()));
        let clone_id = (returned_fork & ATTR_CMNEXT_CLONEID != 0)
            .then(|| u64::from_ne_bytes(take(8).try_into().unwrap()));
        let refcount = (returned_fork & ATTR_CMNEXT_CLONE_REFCNT != 0)
            .then(|| u32::from_ne_bytes(take(4).try_into().unwrap()));
        Ok(private.map(|private| super::Sharing {
            private_bytes: private.max(0) as u64,
            clone_id,
            clone_refcount: refcount,
        }))
    }
}

// ------------------------------------------------------------------- Linux

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::unix::io::AsRawFd;

    const FICLONE: u64 = 0x4004_9409;
    const AT_FDCWD: c_int = -100;
    const RENAME_NOREPLACE: u32 = 1;

    unsafe extern "C" {
        fn ioctl(fd: c_int, request: u64, ...) -> c_int;
        fn renameat2(
            olddirfd: c_int,
            old: *const c_char,
            newdirfd: c_int,
            new: *const c_char,
            flags: u32,
        ) -> c_int;
        fn statvfs(path: *const c_char, buf: *mut c_void) -> c_int;
    }

    /// FICLONE reflink of a regular file; symlinks are recreated by the caller.
    pub fn clone_file(src: &Path, dst: &Path) -> io::Result<()> {
        let source = std::fs::File::open(src)?;
        let dest = OpenOptions::new().write(true).create_new(true).open(dst)?;
        if unsafe { ioctl(dest.as_raw_fd(), FICLONE, source.as_raw_fd()) } == -1 {
            let error = io::Error::last_os_error();
            drop(dest);
            let _ = std::fs::remove_file(dst);
            return Err(error);
        }
        let mode = std::fs::metadata(src)?.permissions();
        std::fs::set_permissions(dst, mode)?;
        let modified = std::fs::metadata(src)?.modified()?;
        dest.set_modified(modified)
    }

    pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
        let (f, t) = (cstr(from)?, cstr(to)?);
        check(unsafe { renameat2(AT_FDCWD, f.as_ptr(), AT_FDCWD, t.as_ptr(), RENAME_NOREPLACE) })
    }

    pub fn filesystem(path: &Path) -> io::Result<super::Filesystem> {
        let p = cstr(path)?;
        let mut buf = [0u8; 256];
        check(unsafe { statvfs(p.as_ptr(), buf.as_mut_ptr().cast()) })?;
        let u64_at = |o: usize| u64::from_ne_bytes(buf[o..o + 8].try_into().unwrap());
        Ok(super::Filesystem {
            available_bytes: u64_at(8) * u64_at(32),
            type_name: String::new(),
        })
    }

    pub fn sharing(_path: &Path) -> io::Result<Option<super::Sharing>> {
        Ok(None)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("clonedir supports macOS (APFS clonefile) and Linux (FICLONE) only");

pub use imp::{clone_file, filesystem, rename_noreplace, sharing};

/// Free space and type of the filesystem holding a path.
#[derive(Debug, Clone)]
pub struct Filesystem {
    /// Bytes available to an unprivileged writer (`f_bavail * f_bsize`).
    pub available_bytes: u64,
    /// `f_fstypename`, e.g. `apfs`; empty where the platform does not report it.
    pub type_name: String,
}

/// APFS extent-sharing attributes of one file (`getattrlist` CMNEXT group).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sharing {
    /// Bytes this file does not share with any other file (`ATTR_CMNEXT_PRIVATESIZE`).
    pub private_bytes: u64,
    /// Equal for files that are still full clones of one another.
    pub clone_id: Option<u64>,
    /// Number of files sharing `clone_id`; meaningful for full clones only.
    pub clone_refcount: Option<u32>,
}

// ----------------------------------------------------- interruption signal

use std::sync::atomic::{AtomicBool, Ordering};

pub static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

unsafe extern "C" {
    fn signal(sig: c_int, handler: extern "C" fn(c_int)) -> usize;
}

/// Route SIGINT and SIGTERM to `INTERRUPTED`, so an operation in progress
/// removes its own stage before the process exits.
pub fn install_interrupt_handler() {
    unsafe {
        signal(2 /* SIGINT */, on_signal);
        signal(15 /* SIGTERM */, on_signal);
    }
}
