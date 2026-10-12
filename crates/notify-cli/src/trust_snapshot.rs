//! Private owned DER trust-set loading; no installed configuration contract.
use crate::{config_snapshot, smtp_provider_revision};
use std::{collections::HashSet, path::PathBuf};

const MAX_ROOTS: usize = 32;
const MAX_BYTES: usize = 65536;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Limit,
    DuplicatePath,
    Read(config_snapshot::Error),
    Trust,
}
// No Debug/Clone; retain the exact validated bytes independently of source paths.
pub(super) struct Trust {
    snapshots: Vec<config_snapshot::Snapshot>,
    fingerprint: String,
}
impl Trust {
    pub(super) fn roots(&self) -> Vec<&[u8]> {
        self.snapshots
            .iter()
            .map(|snapshot| snapshot.bytes())
            .collect()
    }
    pub(super) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}
pub(super) fn load(paths: &[PathBuf]) -> Result<Trust, Error> {
    if paths.is_empty() || paths.len() > MAX_ROOTS {
        return Err(Error::Limit);
    }
    let mut seen = HashSet::new();
    if paths.iter().any(|path| !seen.insert(path)) {
        return Err(Error::DuplicatePath);
    }
    let mut snapshots = Vec::with_capacity(paths.len());
    let mut remaining = MAX_BYTES;
    for path in paths {
        if remaining == 0 {
            return Err(Error::Limit);
        }
        let snapshot = config_snapshot::read_limit(path, remaining).map_err(|error| {
            if error == config_snapshot::Error::Limit {
                Error::Limit
            } else {
                Error::Read(error)
            }
        })?;
        remaining -= snapshot.bytes().len();
        snapshots.push(snapshot);
    }
    let roots: Vec<_> = snapshots.iter().map(|snapshot| snapshot.bytes()).collect();
    let fingerprint =
        smtp_provider_revision::trust_fingerprint(&roots).map_err(|_| Error::Trust)?;
    Ok(Trust {
        snapshots,
        fingerprint,
    })
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    #[allow(unused_imports)] // Custom process harness omits the unit test bodies.
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        os::unix::fs::OpenOptionsExt,
    };
    fn write(directory: &crate::storage::tests::Directory, name: &str, bytes: &[u8]) -> PathBuf {
        let path = directory
            .path()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.write_all(bytes).unwrap();
        path
    }
    fn root(name: &str) -> Vec<u8> {
        rcgen::generate_simple_self_signed(vec![format!("{name}.fixture.invalid")])
            .unwrap()
            .cert
            .der()
            .to_vec()
    }
    #[test]
    fn validated_owned_set_survives_reopen_rotation_and_removal() {
        let directory = crate::storage::tests::Directory::new();
        let first = root("first");
        let second = root("second");
        let a = write(&directory, "a.der", &first);
        let b = write(&directory, "b.der", &second);
        let owned = load(&[a.clone(), b.clone()]).unwrap();
        assert!(owned.roots() == vec![first.as_slice(), second.as_slice()]);
        assert_eq!(
            owned.fingerprint(),
            load(&[b.clone(), a.clone()]).unwrap().fingerprint()
        );
        let rotated = root("rotated");
        fs::write(&a, &rotated).unwrap();
        assert_ne!(
            owned.fingerprint(),
            load(&[a.clone(), b.clone()]).unwrap().fingerprint()
        );
        fs::remove_file(&b).unwrap();
        assert!(owned.roots() == vec![first.as_slice(), second.as_slice()]);
        assert!(matches!(
            load(&[a, b]),
            Err(Error::Read(config_snapshot::Error::Io))
        ));
    }
    #[test]
    fn invalid_sets_fail_without_exposing_resource_content() {
        let directory = crate::storage::tests::Directory::new();
        let bytes = root("valid");
        let a = write(&directory, "a.der", &bytes);
        let b = write(&directory, "b.der", &bytes);
        assert!(matches!(load(&[a.clone(), b]), Err(Error::Trust)));
        let invalid = write(&directory, "invalid.der", b"private-fixture-value");
        assert_eq!(format!("{:?}", load(&[a, invalid]).err().unwrap()), "Trust");
    }
    #[test]
    fn count_duplicate_path_and_aggregate_budget_fail_before_trust_validation() {
        let directory = crate::storage::tests::Directory::new();
        let missing = directory.path().with_file_name("missing.der");
        assert!(matches!(load(&[]), Err(Error::Limit)));
        assert!(matches!(
            load(&vec![missing.clone(); MAX_ROOTS + 1]),
            Err(Error::Limit)
        ));
        assert!(matches!(
            load(&[missing.clone(), missing]),
            Err(Error::DuplicatePath)
        ));
        let a = write(&directory, "a.der", &vec![b'x'; 32768]);
        let b = write(&directory, "b.der", &vec![b'y'; 32768]);
        assert!(matches!(load(&[a.clone(), b.clone()]), Err(Error::Trust)));
        fs::write(&b, vec![b'y'; 32769]).unwrap();
        assert!(matches!(load(&[a.clone(), b.clone()]), Err(Error::Limit)));
        fs::write(&a, vec![b'x'; MAX_BYTES]).unwrap();
        assert!(matches!(load(&[a, b]), Err(Error::Limit)));
    }
}
