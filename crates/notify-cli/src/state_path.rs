//! Test-only private Unix state namespace. No approved product path.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use std::ffi::CString;
    use std::fs::File;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Component, Path, PathBuf};

    #[derive(Debug, PartialEq, Eq)]
    pub(crate) enum Error {
        InvalidPath,
        UnsafePermissions,
        Io,
        AlreadyOwned,
    }

    pub(crate) struct Guard {
        // Keep the directory and advisory lock open for the entire SQLite lifetime.
        _directory: File,
        _lock: File,
        pub(crate) database: PathBuf,
    }

    fn name(value: &std::ffi::OsStr) -> Result<CString, Error> {
        CString::new(value.as_bytes()).map_err(|_| Error::InvalidPath)
    }
    fn open_at(directory: i32, name: &CString, flags: i32, mode: u32) -> Result<File, Error> {
        // SAFETY: CString is NUL-terminated; directory FD remains owned by the caller.
        // All opens reject symlinks and mark descriptors close-on-exec.
        let fd = unsafe {
            libc::openat(
                directory,
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                mode,
            )
        };
        if fd < 0 {
            return Err(Error::Io);
        }
        // SAFETY: successful openat returns a new uniquely owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn check_file(file: &File, owner: u32) -> Result<(), Error> {
        let metadata = file.metadata().map_err(|_| Error::Io)?;
        if !metadata.is_file()
            || metadata.uid() != owner
            || metadata.nlink() != 1
            || metadata.mode() & 0o7777 != 0o600
        {
            return Err(Error::UnsafePermissions);
        }
        Ok(())
    }

    impl Guard {
        pub(crate) fn acquire(path: &Path) -> Result<Self, Error> {
            if !path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
            {
                return Err(Error::InvalidPath);
            }
            let components: Vec<_> = path
                .components()
                .filter_map(|c| match c {
                    Component::Normal(name) => Some(name),
                    _ => None,
                })
                .collect();
            if components.is_empty() {
                return Err(Error::InvalidPath);
            }
            // SAFETY: geteuid has no arguments or memory preconditions.
            let owner = unsafe { libc::geteuid() };
            let mut directory = File::open("/").map_err(|_| Error::Io)?;
            for (index, component) in components.iter().enumerate() {
                let entry = name(component)?;
                let last = index + 1 == components.len();
                if last {
                    // Only the final directory may be created. EEXIST is checked by
                    // descriptor open/metadata, never accepted as proof of safety.
                    // SAFETY: descriptor and CString remain live throughout this call.
                    let result =
                        unsafe { libc::mkdirat(directory.as_raw_fd(), entry.as_ptr(), 0o700) };
                    if result != 0
                        && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
                    {
                        return Err(Error::Io);
                    }
                }
                directory = open_at(
                    directory.as_raw_fd(),
                    &entry,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )?;
                let metadata = directory.metadata().map_err(|_| Error::Io)?;
                let mode = metadata.mode();
                if last {
                    if metadata.uid() != owner || mode & 0o7777 != 0o700 {
                        return Err(Error::UnsafePermissions);
                    }
                } else {
                    let trusted = metadata.uid() == 0 || metadata.uid() == owner;
                    let sticky_root = metadata.uid() == 0 && mode & 0o1000 != 0;
                    if !trusted || mode & 0o022 != 0 && !sticky_root {
                        return Err(Error::UnsafePermissions);
                    }
                }
            }
            let lock_name = CString::new(".owner.lock").unwrap();
            let lock = open_at(
                directory.as_raw_fd(),
                &lock_name,
                libc::O_RDWR | libc::O_CREAT | libc::O_NONBLOCK,
                0o600,
            )?;
            check_file(&lock, owner)?;
            // SAFETY: flock operates on this owned descriptor; LOCK_NB cannot wait.
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(
                    if std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) {
                        Error::AlreadyOwned
                    } else {
                        Error::Io
                    },
                );
            }
            let database_name = CString::new("state.sqlite3").unwrap();
            let database = open_at(
                directory.as_raw_fd(),
                &database_name,
                libc::O_RDWR | libc::O_CREAT | libc::O_NONBLOCK,
                0o600,
            )?;
            check_file(&database, owner)?;
            for suffix in ["-journal", "-wal", "-shm"] {
                let sidecar = CString::new(format!("state.sqlite3{suffix}")).unwrap();
                match open_at(
                    directory.as_raw_fd(),
                    &sidecar,
                    libc::O_RDONLY | libc::O_NONBLOCK,
                    0,
                ) {
                    Ok(file) => check_file(&file, owner)?,
                    Err(_)
                        if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) => {
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(Self {
                _directory: directory,
                _lock: lock,
                database: path.join("state.sqlite3"),
            })
        }
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use unix::{Error, Guard};

#[cfg(test)]
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod tests {
    use crate::storage::tests::Directory;
    fn root(directory: &Directory) -> std::path::PathBuf {
        // temp_dir may itself be a symlink on macOS; callers must resolve it.
        directory
            .path()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join("private-state")
    }

    fn reacquire_after_close(path: &std::path::Path) -> super::Guard {
        // Concurrent suite subprocesses can briefly inherit a flock descriptor
        // between fork and exec despite CLOEXEC. A leak still fails this bound.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            match super::Guard::acquire(path) {
                Ok(guard) => return guard,
                Err(super::Error::AlreadyOwned) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(error) => panic!("reacquire failed: {error:?}"),
            }
        }
    }

    #[test]
    fn creates_private_files_and_releases_owner_on_close() {
        use crate::storage::Store;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Duration;
        let directory = Directory::new();
        let path = root(&directory);
        let store = Store::open_private(&path, Duration::from_millis(50)).unwrap();
        for (name, mode) in [
            ("", 0o700),
            ("state.sqlite3", 0o600),
            (".owner.lock", 0o600),
        ] {
            assert_eq!(
                std::fs::metadata(path.join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777,
                mode
            );
        }
        assert!(matches!(
            Store::open_private(&path, Duration::from_millis(50)),
            Err(crate::storage::StorageError::StatePath(
                super::Error::AlreadyOwned
            ))
        ));
        drop(store);
        drop(reacquire_after_close(&path));
    }
    #[test]
    fn rejects_symlinks_in_directory_database_lock_and_sidecars() {
        use super::Guard;
        use std::os::unix::fs::symlink;
        for name in [
            "state.sqlite3",
            ".owner.lock",
            "state.sqlite3-journal",
            "state.sqlite3-wal",
            "state.sqlite3-shm",
        ] {
            let directory = Directory::new();
            let path = root(&directory);
            drop(Guard::acquire(&path).unwrap());
            let target = path.join(name);
            let _ = std::fs::remove_file(&target);
            symlink(directory.path(), &target).unwrap();
            assert!(Guard::acquire(&path).is_err());
        }
        let directory = Directory::new();
        let path = root(&directory);
        symlink(path.parent().unwrap(), &path).unwrap();
        assert!(Guard::acquire(&path).is_err());
    }
    #[test]
    fn rejects_broad_permissions_hard_links_and_relative_paths() {
        use super::Guard;
        use std::os::unix::fs::PermissionsExt;
        let directory = Directory::new();
        let path = root(&directory);
        drop(Guard::acquire(&path).unwrap());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Guard::acquire(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let database = path.join("state.sqlite3");
        std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Guard::acquire(&path).is_err());
        std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(&database, path.join("alias")).unwrap();
        assert!(Guard::acquire(&path).is_err());
        assert!(Guard::acquire(std::path::Path::new("relative")).is_err());
    }
    #[test]
    fn failed_database_initialization_releases_lock() {
        use super::Guard;
        use crate::storage::Store;
        use std::time::Duration;
        let directory = Directory::new();
        let path = root(&directory);
        drop(Guard::acquire(&path).unwrap());
        std::fs::write(path.join("state.sqlite3"), b"invalid database fixture").unwrap();
        assert!(Store::open_private(&path, Duration::from_millis(50)).is_err());
        drop(reacquire_after_close(&path));
    }

    #[test]
    fn killed_owner_does_not_leave_a_stale_lock() {
        use super::Guard;
        use std::time::Duration;
        let directory = Directory::new();
        let path = root(&directory);
        drop(Guard::acquire(&path).unwrap());
        let marker = path.join("owner-started");
        let mut command = std::process::Command::new("python3");
        command.args(["-c", "import fcntl,sys,time; f=open(sys.argv[1],'r+b'); fcntl.flock(f,fcntl.LOCK_EX); open(sys.argv[2],'w').close(); time.sleep(30)"])
            .arg(path.join(".owner.lock")).arg(&marker);
        assert_eq!(
            crate::transport::supervise(&mut command, vec![], Duration::from_millis(300), 16, 32),
            Err(crate::transport::Error::Timeout)
        );
        assert!(
            marker.exists(),
            "fixture must acquire the lock before termination"
        );
        drop(reacquire_after_close(&path));
    }

    #[test]
    fn independent_process_cannot_acquire_owned_lock() {
        use super::Guard;
        use std::time::Duration;
        let directory = Directory::new();
        let path = root(&directory);
        let guard = Guard::acquire(&path).unwrap();
        let script = "import fcntl,sys; f=open(sys.argv[1],'r+b');\ntry: fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB); print('OK')\nexcept BlockingIOError: print('REJECT')";
        let mut command = std::process::Command::new("python3");
        command.args(["-c", script]).arg(path.join(".owner.lock"));
        assert_eq!(
            crate::transport::supervise(&mut command, vec![], Duration::from_secs(2), 16, 32)
                .unwrap(),
            crate::delivery::ProviderOutcome::Rejected
        );
        drop(guard);
        let wait_script = "import fcntl,sys; f=open(sys.argv[1],'r+b'); fcntl.flock(f,fcntl.LOCK_EX); print('OK')";
        let mut command = std::process::Command::new("python3");
        command
            .args(["-c", wait_script])
            .arg(path.join(".owner.lock"));
        assert_eq!(
            crate::transport::supervise(&mut command, vec![], Duration::from_secs(2), 16, 32)
                .unwrap(),
            crate::delivery::ProviderOutcome::Delivered
        );
    }
}
