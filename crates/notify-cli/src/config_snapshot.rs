//! Private configuration byte snapshot experiment; no installed path or JSON contract.
use std::io::Read;
use zeroize::Zeroizing;

const MAX_BYTES: usize = 65536;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Io,
    Limit,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    InvalidPath,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    UnsafePermissions,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Changed,
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    UnsupportedPlatform,
}
// No Debug/Clone; snapshots may contain private metadata or rejected inline secrets.
pub(super) struct Snapshot(Zeroizing<Vec<u8>>);
impl Snapshot {
    pub(super) fn bytes(&self) -> &[u8] {
        &self.0
    }
}
fn bounded(input: impl Read) -> Result<Snapshot, Error> {
    bounded_limit(input, MAX_BYTES)
}
fn bounded_limit(mut input: impl Read, limit: usize) -> Result<Snapshot, Error> {
    if limit == 0 || limit > MAX_BYTES {
        return Err(Error::Limit);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    let mut block = Zeroizing::new([0; 4096]);
    loop {
        let remaining = (limit + 1 - bytes.len()).min(block.len());
        let size = match input.read(&mut block[..remaining]) {
            Ok(size) => size,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(Error::Io),
        };
        if size == 0 {
            return if bytes.is_empty() {
                Err(Error::Limit)
            } else {
                Ok(Snapshot(bytes))
            };
        }
        bytes.extend_from_slice(&block[..size]);
        if bytes.len() > limit {
            return Err(Error::Limit);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use super::*;
    use std::{
        ffi::CString,
        fs::{File, Metadata},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::MetadataExt},
        },
        path::{Component, Path},
    };
    fn open_at(
        directory: &File,
        name: &std::ffi::OsStr,
        directory_only: bool,
    ) -> Result<File, Error> {
        let name = CString::new(name.as_bytes()).map_err(|_| Error::InvalidPath)?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if directory_only { libc::O_DIRECTORY } else { 0 };
        // SAFETY: live parent descriptor and NUL-terminated name; no create mode is needed.
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(Error::Io);
        }
        // SAFETY: openat returned a new descriptor exclusively owned by this File.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn file_metadata(file: &File, owner: u32) -> Result<Metadata, Error> {
        let metadata = file.metadata().map_err(|_| Error::Io)?;
        if !metadata.is_file()
            || metadata.uid() != owner
            || metadata.nlink() != 1
            || metadata.mode() & 0o7777 != 0o600
        {
            return Err(Error::UnsafePermissions);
        }
        Ok(metadata)
    }
    fn same(before: &Metadata, after: &Metadata) -> bool {
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.len() == after.len()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
    }
    pub(super) fn read_with(
        path: &Path,
        read: impl FnOnce(&mut File) -> Result<Snapshot, Error>,
    ) -> Result<Snapshot, Error> {
        let raw = path.as_os_str().as_bytes();
        if !path.is_absolute()
            || path.to_str().is_none()
            || raw.len() > 4096
            || raw.ends_with(b"/")
            || raw.iter().any(|b| *b < 32 || *b == 127)
            || raw
                .split(|b| *b == b'/')
                .any(|part| part == b"." || part == b"..")
        {
            return Err(Error::InvalidPath);
        }
        let parts: Vec<_> = path
            .components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(name),
                _ => None,
            })
            .collect();
        if parts.len() < 2 {
            return Err(Error::InvalidPath);
        }
        // SAFETY: geteuid has no arguments or memory preconditions.
        let owner = unsafe { libc::geteuid() };
        let mut directory = File::open("/").map_err(|_| Error::Io)?;
        for (index, part) in parts[..parts.len() - 1].iter().enumerate() {
            directory = open_at(&directory, part, true)?;
            let metadata = directory.metadata().map_err(|_| Error::Io)?;
            let mode = metadata.mode();
            if index + 2 == parts.len() {
                if metadata.uid() != owner || mode & 0o7777 != 0o700 {
                    return Err(Error::UnsafePermissions);
                }
            } else {
                let trusted = metadata.uid() == owner || metadata.uid() == 0;
                let sticky_root = metadata.uid() == 0 && mode & 0o1000 != 0;
                if !trusted || mode & 0o022 != 0 && !sticky_root {
                    return Err(Error::UnsafePermissions);
                }
            }
        }
        let mut file = open_at(&directory, parts[parts.len() - 1], false)?;
        let before = file_metadata(&file, owner)?;
        if before.len() == 0 || before.len() > MAX_BYTES as u64 {
            return Err(Error::Limit);
        }
        let snapshot = read(&mut file)?;
        let after = file_metadata(&file, owner)?;
        if !same(&before, &after) || snapshot.bytes().len() as u64 != before.len() {
            return Err(Error::Changed);
        }
        Ok(snapshot)
    }
    pub(super) fn read(path: &Path) -> Result<Snapshot, Error> {
        read_with(path, |file| bounded(file))
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn read(path: &std::path::Path) -> Result<Snapshot, Error> {
    unix::read(path)
}
pub(super) fn read_limit(path: &std::path::Path, limit: usize) -> Result<Snapshot, Error> {
    if limit == 0 || limit > MAX_BYTES {
        return Err(Error::Limit);
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::read_with(path, |file| bounded_limit(file, limit))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = path;
        Err(Error::UnsupportedPlatform)
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn read(_: &std::path::Path) -> Result<Snapshot, Error> {
    Err(Error::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness compiles helpers without unit tests.
    use super::*;
    #[test]
    fn stream_budget_counts_actual_reads_and_preserves_bytes() {
        assert!(matches!(bounded(&b""[..]), Err(Error::Limit)));
        let bytes = vec![b'x'; MAX_BYTES];
        assert!(bounded(bytes.as_slice()).unwrap().bytes() == bytes);
        assert!(matches!(
            bounded(vec![b'x'; MAX_BYTES + 1].as_slice()),
            Err(Error::Limit)
        ));
        struct Source {
            count: usize,
        }
        impl Read for Source {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                self.count += output.len();
                output.fill(b'x');
                Ok(output.len())
            }
        }
        let mut source = Source { count: 0 };
        assert!(matches!(bounded(&mut source), Err(Error::Limit)));
        assert_eq!(source.count, MAX_BYTES + 1);
        source.count = 0;
        assert!(matches!(bounded_limit(&mut source, 7), Err(Error::Limit)));
        assert_eq!(source.count, 8);
        assert!(matches!(bounded_limit(&b"x"[..], 0), Err(Error::Limit)));
        assert!(matches!(
            bounded_limit(&b"x"[..], MAX_BYTES + 1),
            Err(Error::Limit)
        ));
    }
    #[test]
    fn io_failure_and_interruption_have_static_categories() {
        struct Source(u8);
        impl Read for Source {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                self.0 += 1;
                match self.0 {
                    1 => Err(std::io::ErrorKind::Interrupted.into()),
                    2 => {
                        output[0] = b'x';
                        Ok(1)
                    }
                    _ => Err(std::io::Error::other("private-fixture-value")),
                }
            }
        }
        assert_eq!(format!("{:?}", bounded(Source(0)).err().unwrap()), "Io");
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    mod files {
        #[allow(unused_imports)] // Custom process harness omits the unit test bodies.
        use super::*;
        #[allow(unused_imports)] // Custom process harness omits the unit test bodies.
        use std::{
            fs::{self, OpenOptions, Permissions},
            io::Write,
            os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt},
        };
        fn fixture() -> (crate::storage::tests::Directory, std::path::PathBuf) {
            let directory = crate::storage::tests::Directory::new();
            let path = directory
                .path()
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap()
                .join("config.json");
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .unwrap();
            file.write_all(include_bytes!(
                "../../../docs/proposals/examples/local-mixed-config.json"
            ))
            .unwrap();
            (directory, path)
        }
        #[test]
        fn private_file_snapshot_survives_reopen_and_is_immutable_after_replacement() {
            let (_directory, path) = fixture();
            let saved = read(&path).unwrap();
            assert!(
                saved.bytes()
                    == include_bytes!("../../../docs/proposals/examples/local-mixed-config.json")
            );
            assert!(read(&path).unwrap().bytes() == saved.bytes());
            fs::write(&path, b"changed").unwrap();
            assert!(saved.bytes() != read(&path).unwrap().bytes());
            fs::remove_file(&path).unwrap();
            assert!(saved.bytes().starts_with(b"{"));
            assert!(matches!(read(&path), Err(Error::Io)));
        }
        #[test]
        fn rejects_unsafe_paths_permissions_links_and_special_files_without_blocking() {
            let (_directory, path) = fixture();
            for bad in [
                std::path::PathBuf::from("relative"),
                path.parent().unwrap().join("../config.json"),
                path.parent().unwrap().join("./config.json"),
                std::path::PathBuf::from("/"),
                std::path::PathBuf::from(format!("/{}", "x".repeat(4096))),
            ] {
                assert!(matches!(read(&bad), Err(Error::InvalidPath)));
            }
            fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
            assert!(matches!(read(&path), Err(Error::UnsafePermissions)));
            fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
            fs::set_permissions(path.parent().unwrap(), Permissions::from_mode(0o755)).unwrap();
            assert!(matches!(read(&path), Err(Error::UnsafePermissions)));
            fs::set_permissions(path.parent().unwrap(), Permissions::from_mode(0o700)).unwrap();
            let linked = path.with_file_name("linked");
            fs::hard_link(&path, &linked).unwrap();
            assert!(matches!(read(&path), Err(Error::UnsafePermissions)));
            fs::remove_file(&linked).unwrap();
            symlink(&path, &linked).unwrap();
            assert!(matches!(read(&linked), Err(Error::Io)));
            fs::remove_file(&linked).unwrap();
            symlink(path.parent().unwrap(), &linked).unwrap();
            assert!(matches!(read(&linked.join("config.json")), Err(Error::Io)));
            fs::remove_file(&linked).unwrap();
            fs::create_dir(&linked).unwrap();
            assert!(matches!(read(&linked), Err(Error::UnsafePermissions)));
            fs::remove_dir(&linked).unwrap();
            let name = std::ffi::CString::new(linked.as_os_str().as_encoded_bytes()).unwrap();
            // SAFETY: valid NUL-terminated owned temporary path, creating only a fixture FIFO.
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            assert!(matches!(read(&linked), Err(Error::UnsafePermissions)));
        }
        #[test]
        fn atomic_replacement_rejects_old_descriptor_and_reopen_reads_new_file() {
            let (_directory, path) = fixture();
            let replacement = path.with_file_name("replacement");
            let mut next = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&replacement)
                .unwrap();
            next.write_all(b"replacement bytes").unwrap();
            assert!(matches!(
                unix::read_with(&path, |file| {
                    fs::rename(&replacement, &path).unwrap();
                    bounded(file)
                }),
                Err(Error::UnsafePermissions)
            ));
            assert_eq!(read(&path).unwrap().bytes(), b"replacement bytes");
        }
        #[test]
        fn symlink_replacement_never_publishes_or_follows_replacement() {
            let (_directory, path) = fixture();
            let target = path.with_file_name("target");
            fs::write(&target, b"replacement target").unwrap();
            assert!(matches!(
                unix::read_with(&path, |file| {
                    let snapshot = bounded(file)?;
                    fs::remove_file(&path).unwrap();
                    symlink(&target, &path).unwrap();
                    Ok(snapshot)
                }),
                Err(Error::UnsafePermissions)
            ));
            assert!(matches!(read(&path), Err(Error::Io)));
            assert_eq!(fs::read(&target).unwrap(), b"replacement target");
        }
        #[test]
        fn same_size_rewrite_and_permission_change_reject_collected_bytes() {
            let (_directory, path) = fixture();
            assert!(matches!(
                unix::read_with(&path, |file| {
                    let snapshot = bounded(file)?;
                    let mut writer = OpenOptions::new().write(true).open(&path).unwrap();
                    writer
                        .write_all(&vec![b'x'; snapshot.bytes().len()])
                        .unwrap();
                    // Force a distinct timestamp without relying on filesystem clock resolution.
                    writer
                        .set_times(std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
                        .unwrap();
                    Ok(snapshot)
                }),
                Err(Error::Changed)
            ));
            assert!(read(&path)
                .unwrap()
                .bytes()
                .iter()
                .all(|byte| *byte == b'x'));
            assert!(matches!(
                unix::read_with(&path, |file| {
                    let snapshot = bounded(file)?;
                    fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
                    Ok(snapshot)
                }),
                Err(Error::UnsafePermissions)
            ));
            assert!(matches!(read(&path), Err(Error::UnsafePermissions)));
        }
        #[test]
        fn size_and_metadata_changes_fail_without_publishing_partial_snapshot() {
            let (_directory, path) = fixture();
            assert!(matches!(read_limit(&path, 0), Err(Error::Limit)));
            assert!(matches!(
                read_limit(&path, MAX_BYTES + 1),
                Err(Error::Limit)
            ));
            assert!(matches!(read_limit(&path, 7), Err(Error::Limit)));
            assert!(matches!(
                unix::read_with(&path, |file| {
                    let snapshot = bounded(file)?;
                    OpenOptions::new()
                        .append(true)
                        .open(&path)
                        .unwrap()
                        .write_all(b"changed")
                        .unwrap();
                    Ok(snapshot)
                }),
                Err(Error::Changed)
            ));
            fs::write(&path, vec![b'x'; MAX_BYTES + 1]).unwrap();
            assert!(matches!(read(&path), Err(Error::Limit)));
            fs::write(&path, b"").unwrap();
            assert!(matches!(read(&path), Err(Error::Limit)));
            fs::write(&path, vec![b'x'; MAX_BYTES]).unwrap();
            assert_eq!(read(&path).unwrap().bytes().len(), MAX_BYTES);
        }
    }
    #[test]
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn unsupported_platform_does_not_access_path() {
        assert!(matches!(
            read(std::path::Path::new("fixture")),
            Err(Error::UnsupportedPlatform)
        ));
    }
}
