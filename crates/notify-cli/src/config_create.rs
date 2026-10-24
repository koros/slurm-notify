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
pub(super) enum Error {
    Destination,
    Io,
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
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| Error::Io)?;
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
    if renamed != 0 {
        return Err(Error::Io);
    }
    temporary.created = false;
    temporary.directory.sync_all().map_err(|_| Error::Io)?;
    Ok(())
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
