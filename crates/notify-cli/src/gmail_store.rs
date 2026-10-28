//! Credential-level exclusive lease and same-directory atomic grant replacement.
//! Cooperating processes share this lock across aliases and state directories.
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, PermissionsExt},
        },
    },
    path::Path,
};
use zeroize::Zeroizing;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum AcquireError {
    Busy,
    Unavailable,
}
pub(super) struct Lease {
    directory: File,
    name: CString,
    _lock: File,
}
struct Temporary<'a> {
    directory: &'a File,
    name: CString,
}
impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        // SAFETY: the name is unique to this operation in the owned parent.
        unsafe {
            libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0);
        }
    }
}
impl Lease {
    pub(super) fn acquire(path: &Path) -> Result<Self, AcquireError> {
        let (directory, name) = crate::config_snapshot::protected_parent(path)
            .map_err(|_| AcquireError::Unavailable)?;
        let name = CString::new(name.as_bytes()).map_err(|_| AcquireError::Unavailable)?;
        let lock_name = CString::new(format!(
            ".{}.gmail.lock",
            name.to_str().map_err(|_| AcquireError::Unavailable)?
        ))
        .map_err(|_| AcquireError::Unavailable)?;
        // SAFETY: live descriptor and NUL-terminated name; reject links and inherited FDs.
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                lock_name.as_ptr(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(AcquireError::Unavailable);
        }
        // SAFETY: successful openat returned a new owned descriptor.
        let lock = unsafe { File::from_raw_fd(fd) };
        let m = lock.metadata().map_err(|_| AcquireError::Unavailable)?;
        // SAFETY: geteuid has no preconditions.
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.nlink() != 1
            || m.mode() & 0o7777 != 0o600
        {
            return Err(AcquireError::Unavailable);
        }
        // SAFETY: nonblocking flock on an owned descriptor.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
                    AcquireError::Busy
                } else {
                    AcquireError::Unavailable
                },
            );
        }
        Ok(Self {
            directory,
            name,
            _lock: lock,
        })
    }
    pub(super) fn read(&self) -> Result<Zeroizing<Vec<u8>>, ()> {
        // SAFETY: anchored name and parent remain owned; links and inherited FDs are refused.
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                self.name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(());
        }
        // SAFETY: successful openat returned a new owned descriptor.
        let file = unsafe { File::from_raw_fd(fd) };
        let metadata = file.metadata().map_err(|_| ())?;
        // SAFETY: geteuid has no arguments or preconditions.
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.nlink() != 1
            || metadata.mode() & 0o7777 != 0o600
            || metadata.len() > 16384
        {
            return Err(());
        }
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(16385).read_to_end(&mut bytes).map_err(|_| ())?;
        if bytes.is_empty() || bytes.len() > 16384 {
            return Err(());
        }
        Ok(bytes)
    }
    pub(super) fn remove(&self, expected: &[u8]) -> Result<(), ()> {
        if self.read()?.as_slice() != expected {
            return Err(());
        }
        // SAFETY: exact expected protected file under the credential lease and owned parent.
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0) } != 0 {
            return Err(());
        }
        self.directory.sync_all().map_err(|_| ())
    }
    pub(super) fn replace(&self, expected: &[u8], replacement: &[u8]) -> Result<(), ()> {
        self.replace_using(expected, replacement, File::sync_all)
    }
    fn replace_using(
        &self,
        expected: &[u8],
        replacement: &[u8],
        mut sync: impl FnMut(&File) -> std::io::Result<()>,
    ) -> Result<(), ()> {
        if replacement.is_empty()
            || replacement.len() > 16384
            || self.read()?.as_slice() != expected
        {
            return Err(());
        }
        let random = crate::public_event::identifier(crate::public_event::random()?);
        let temporary = Temporary {
            directory: &self.directory,
            name: CString::new(format!(".gmail-grant-{random}")).map_err(|_| ())?,
        };
        // SAFETY: exclusive new file relative to owned directory; no links followed.
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                temporary.name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(());
        }
        // SAFETY: successful exclusive open creates a uniquely owned descriptor.
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|_| ())?;
        file.write_all(replacement).map_err(|_| ())?;
        sync(&file).map_err(|_| ())?;
        if self.read()?.as_slice() != expected {
            return Err(());
        }
        // SAFETY: both names use the same owned parent. The credential lease
        // serializes cooperating writers; expected contents were checked above.
        if unsafe {
            libc::renameat(
                self.directory.as_raw_fd(),
                temporary.name.as_ptr(),
                self.directory.as_raw_fd(),
                self.name.as_ptr(),
            )
        } != 0
        {
            return Err(());
        }
        sync(&self.directory).map_err(|_| ())
    }
}
#[cfg(test)]
mod tests {
    fn reacquire(path: &std::path::Path) -> super::Lease {
        // A parallel suite child can briefly inherit the CLOEXEC lock descriptor
        // between fork and exec. Only lock contention may wait; leaks still fail.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            match super::Lease::acquire(path) {
                Ok(lease) => return lease,
                Err(super::AcquireError::Busy) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(2))
                }
                Err(error) => panic!("credential reacquisition failed: {error:?}"),
            }
        }
    }
    #[test]
    fn exclusive_atomic_rotation_rejects_stale_inputs_and_preserves_old_on_write_failure() {
        use std::os::unix::fs::PermissionsExt;
        let d = crate::storage::tests::Directory::new();
        let path = d
            .path()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join("grant.json");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let lease = super::Lease::acquire(&path).unwrap();
        assert!(matches!(
            super::Lease::acquire(&path),
            Err(super::AcquireError::Busy)
        ));
        assert!(lease.replace(b"wrong", b"new").is_err());
        assert!(lease
            .replace_using(b"old", b"new", |_| Err(std::io::Error::other("fixture")))
            .is_err());
        assert_eq!(lease.read().unwrap().as_slice(), b"old");
        lease.replace(b"old", b"new").unwrap();
        assert_eq!(lease.read().unwrap().as_slice(), b"new");
        assert!(lease.replace(b"old", b"next").is_err());
        let mut calls = 0;
        assert!(lease
            .replace_using(b"new", b"published", |file| {
                calls += 1;
                if calls == 2 {
                    Err(std::io::Error::other("directory sync fixture"))
                } else {
                    file.sync_all()
                }
            })
            .is_err());
        assert_eq!(lease.read().unwrap().as_slice(), b"published");

        drop(lease);
        let lease = reacquire(&path);
        assert_eq!(lease.read().unwrap().as_slice(), b"published");
        drop(lease);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(reacquire(&path).read().is_err());
    }
    #[test]
    fn unsafe_lock_is_unavailable_rather_than_retryable_contention() {
        use std::os::unix::fs::PermissionsExt;
        let d = crate::storage::tests::Directory::new();
        let root = d.path().parent().unwrap().canonicalize().unwrap();
        let path = root.join("grant.json");
        drop(super::Lease::acquire(&path).unwrap());
        std::fs::set_permissions(
            root.join(".grant.json.gmail.lock"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(matches!(
            super::Lease::acquire(&path),
            Err(super::AcquireError::Unavailable)
        ));
    }
}
