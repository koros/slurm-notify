//! Protected same-directory publication, atomically refusing every existing target.
use std::{
    ffi::CString,
    fs::File,
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::PermissionsExt},
    },
    path::Path,
};
#[derive(Debug)]
pub(super) enum Error {
    Destination,
    Io,
    FileSync,
    Publication,
    PublishedCleanup,
    PublishedSync,
}
fn parent(path: &Path) -> Result<(File, CString), Error> {
    let (dir, name) =
        crate::config_snapshot::protected_parent(path).map_err(|_| Error::Destination)?;
    Ok((
        dir,
        CString::new(name.as_bytes()).map_err(|_| Error::Destination)?,
    ))
}
pub(super) fn check(path: &Path) -> Result<(), Error> {
    let (dir, name) = parent(path)?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: live directory/name and writable stat storage; do not follow symlinks.
    let result = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT) {
        return Err(Error::Destination);
    }
    Ok(())
}
struct Temporary {
    directory: File,
    name: CString,
    created: bool,
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if !self.created {
            return;
        }
        // SAFETY: owned parent/name; cleanup never touches the published target.
        let _ = unsafe { libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0) };
    }
}
pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    write_using(path, bytes, rename_noreplace)
}
fn write_using(
    path: &Path,
    bytes: &[u8],
    rename: impl FnOnce(&Temporary, &CString) -> std::io::Result<()>,
) -> Result<(), Error> {
    write_steps(path, bytes, rename, File::sync_all)
}
fn write_steps(
    path: &Path,
    bytes: &[u8],
    rename: impl FnOnce(&Temporary, &CString) -> std::io::Result<()>,
    mut sync: impl FnMut(&File) -> std::io::Result<()>,
) -> Result<(), Error> {
    if bytes.is_empty() || bytes.len() > 65536 {
        return Err(Error::Destination);
    }
    let (directory, target) = parent(path)?;
    let random =
        crate::public_event::identifier(crate::public_event::random().map_err(|_| Error::Io)?);
    let name = CString::new(format!(".slurm-notify-init-{random}")).map_err(|_| Error::Io)?;
    let mut temporary = Temporary {
        directory,
        name,
        created: false,
    };
    // SAFETY: live descriptor/name and explicit create mode. No existing entry is opened.
    let fd = unsafe {
        libc::openat(
            temporary.directory.as_raw_fd(),
            temporary.name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(Error::Io);
    }
    temporary.created = true;
    // SAFETY: newly created descriptor is exclusively owned by this File.
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|_| Error::Io)?;
    file.write_all(bytes).map_err(|_| Error::Io)?;
    sync(&file).map_err(|_| Error::FileSync)?;
    publish(&mut temporary, &target, rename)?;
    sync(&temporary.directory).map_err(|_| Error::PublishedSync)?;
    Ok(())
}
fn rename_noreplace(temporary: &Temporary, target: &CString) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    // SAFETY: both names are relative to the same owned directory; NOREPLACE is atomic.
    let renamed = unsafe {
        libc::renameat2(
            temporary.directory.as_raw_fd(),
            temporary.name.as_ptr(),
            temporary.directory.as_raw_fd(),
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    // SAFETY: same owned directory and NUL-terminated names; EXCL forbids replacement.
    let renamed = unsafe {
        libc::renameatx_np(
            temporary.directory.as_raw_fd(),
            temporary.name.as_ptr(),
            temporary.directory.as_raw_fd(),
            target.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if renamed == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
fn publish(
    temporary: &mut Temporary,
    target: &CString,
    rename: impl FnOnce(&Temporary, &CString) -> std::io::Result<()>,
) -> Result<(), Error> {
    match rename(temporary, target) {
        Ok(()) => {
            temporary.created = false;
            Ok(())
        }
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(libc::ENOSYS) | Some(libc::EINVAL) | Some(libc::EOPNOTSUPP)
            ) =>
        {
            // SAFETY: both names are relative to the owned protected directory.
            // linkat atomically refuses an existing target; it never replaces it.
            if unsafe {
                libc::linkat(
                    temporary.directory.as_raw_fd(),
                    temporary.name.as_ptr(),
                    temporary.directory.as_raw_fd(),
                    target.as_ptr(),
                    0,
                )
            } != 0
            {
                return Err(Error::Publication);
            }
            // Remove the original link before reporting success: protected readers
            // require exactly one link. A crash here is detected as unsafe state.
            // SAFETY: this unlinks only the temporary name created by this operation.
            if unsafe {
                libc::unlinkat(temporary.directory.as_raw_fd(), temporary.name.as_ptr(), 0)
            } != 0
            {
                return Err(Error::PublishedCleanup);
            }
            temporary.created = false;
            Ok(())
        }
        Err(_) => Err(Error::Publication),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publish_is_private_complete_and_never_replaces_files_or_symlinks() {
        use std::os::unix::fs::{symlink, MetadataExt};
        let dir = crate::storage::tests::Directory::new();
        let root = dir.path().parent().unwrap().canonicalize().unwrap();
        let path = root.join("created.json");
        assert!(check(&path).is_ok());
        write_new(&path, b"{}\n").ok().unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(meta.mode() & 0o7777, 0o600);
        assert_eq!(meta.nlink(), 1);
        assert!(check(&path).is_err());
        assert!(write_new(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{}\n");
        let link = root.join("link.json");
        symlink(&path, &link).unwrap();
        assert!(check(&link).is_err());
        assert!(write_new(&link, b"replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{}\n");
        assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".slurm-notify-init-")));
        assert!(check(&root.join("absent/config.json")).is_err());
        assert!(!root.join("absent").exists());
        assert!(write_new(&root.join("empty"), b"").is_err());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(check(&root.join("unsafe")).is_err());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn unsupported_rename_falls_back_to_private_single_link_publication() {
        use std::os::unix::fs::{symlink, MetadataExt};
        let dir = crate::storage::tests::Directory::new();
        let root = dir.path().parent().unwrap().canonicalize().unwrap();
        for (index, code) in [libc::ENOSYS, libc::EINVAL, libc::EOPNOTSUPP]
            .into_iter()
            .enumerate()
        {
            let path = root.join(format!("fallback-{index}.json"));
            let fallback =
                |_: &Temporary, _: &CString| Err(std::io::Error::from_raw_os_error(code));
            assert!(write_using(&path, b"{}\n", fallback).is_ok());
            assert_eq!(
                crate::config_snapshot::read(&path).unwrap().bytes(),
                b"{}\n"
            );
            let metadata = std::fs::metadata(&path).unwrap();
            assert_eq!(metadata.nlink(), 1);
            assert_eq!(metadata.mode() & 0o7777, 0o600);
            assert!(matches!(
                write_using(&path, b"replacement", fallback),
                Err(Error::Publication)
            ));
            let link = root.join(format!("symlink-{index}"));
            symlink(&path, &link).unwrap();
            assert!(write_using(&link, b"replacement", fallback).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"{}\n");
        }
        assert!(!std::fs::read_dir(root).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".slurm-notify-init-")));
    }
    #[test]
    fn publication_failures_keep_sync_guarantees_and_report_stage() {
        let dir = crate::storage::tests::Directory::new();
        let root = dir.path().parent().unwrap().canonicalize().unwrap();
        let failed = root.join("failed.json");
        assert!(matches!(
            write_using(&failed, b"{}", |_, _| Err(
                std::io::Error::from_raw_os_error(libc::EACCES)
            )),
            Err(Error::Publication)
        ));
        assert!(!failed.exists());
        assert!(matches!(
            write_steps(
                &failed,
                b"{}",
                |_, _| panic!("must not publish without file sync"),
                |_| Err(std::io::Error::from_raw_os_error(libc::EIO))
            ),
            Err(Error::FileSync)
        ));
        assert!(!failed.exists());
        let mut calls = 0;
        assert!(matches!(
            write_steps(&failed, b"{}", rename_noreplace, |file| {
                calls += 1;
                if calls == 1 {
                    file.sync_all()
                } else {
                    Err(std::io::Error::from_raw_os_error(libc::EIO))
                }
            }),
            Err(Error::PublishedSync)
        ));
        assert_eq!(
            crate::config_snapshot::read(&failed).unwrap().bytes(),
            b"{}"
        );
        assert!(write_new(&failed, b"replacement").is_err());
        assert_eq!(std::fs::read(&failed).unwrap(), b"{}");
    }
    #[test]
    fn interrupted_link_publication_remains_blocked_until_extra_link_is_removed() {
        let dir = crate::storage::tests::Directory::new();
        let root = dir.path().parent().unwrap().canonicalize().unwrap();
        let temporary = root.join("interrupted-temp");
        let target = root.join("interrupted.json");
        std::fs::write(&temporary, b"{}").unwrap();
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(&temporary, &target).unwrap();
        assert_eq!(
            crate::config_snapshot::read(&target).err(),
            Some(crate::config_snapshot::Error::UnsafePermissions)
        );
        assert!(write_new(&target, b"replacement").is_err());
        assert_eq!(std::fs::read(&temporary).unwrap(), b"{}");
        // Model deliberate recovery of the known extra name after the owner stops.
        std::fs::remove_file(temporary).unwrap();
        assert_eq!(
            crate::config_snapshot::read(&target).unwrap().bytes(),
            b"{}"
        );
    }
    #[test]
    fn concurrent_fallback_has_one_complete_winner() {
        let dir = crate::storage::tests::Directory::new();
        let root = dir.path().parent().unwrap().canonicalize().unwrap();
        let path = root.join("fallback-race.json");
        let mut tasks = Vec::new();
        for bytes in [b"first".as_slice(), b"second"] {
            let path = path.clone();
            tasks.push(std::thread::spawn(move || {
                write_using(&path, bytes, |_, _| {
                    Err(std::io::Error::from_raw_os_error(libc::EOPNOTSUPP))
                })
                .is_ok()
            }));
        }
        assert_eq!(
            tasks
                .into_iter()
                .map(|task| task.join().unwrap())
                .filter(|won| *won)
                .count(),
            1
        );
        let data = crate::config_snapshot::read(&path).unwrap();
        assert!([b"first".as_slice(), b"second"].contains(&data.bytes()));
    }
    #[test]
    fn concurrent_publish_has_one_complete_winner() {
        let dir = crate::storage::tests::Directory::new();
        let path = dir
            .path()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join("race.json");
        let mut tasks = Vec::new();
        for bytes in [b"first".as_slice(), b"second"] {
            let path = path.clone();
            tasks.push(std::thread::spawn(move || write_new(&path, bytes).is_ok()));
        }
        assert_eq!(
            tasks
                .into_iter()
                .map(|task| task.join().unwrap())
                .filter(|won| *won)
                .count(),
            1
        );
        assert!([b"first".as_slice(), b"second"].contains(&std::fs::read(path).unwrap().as_slice()));
    }
}
