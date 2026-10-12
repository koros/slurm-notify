//! Private Linux/macOS effective-UID lookup; no environment identity fallback.
use crate::observation_mapping::valid_identity;
use zeroize::Zeroizing;

const SCRATCH_BYTES: usize = 65_536;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Limit,
    Lookup,
    Missing,
    Identity,
}

pub(super) fn lookup(ceiling: usize) -> Result<Zeroizing<Vec<u8>>, ()> {
    read_with(ceiling, os_name).map_err(|_| ())
}

fn read_with(
    ceiling: usize,
    source: impl FnOnce(&mut [u8]) -> Result<Option<usize>, Error>,
) -> Result<Zeroizing<Vec<u8>>, Error> {
    if !(1..=256).contains(&ceiling) {
        return Err(Error::Limit);
    }
    let mut scratch = Zeroizing::new(vec![0; SCRATCH_BYTES]);
    let offset = source(&mut scratch)?.ok_or(Error::Missing)?;
    let tail = scratch.get(offset..).ok_or(Error::Identity)?;
    let end = tail
        .iter()
        .take(ceiling + 1)
        .position(|byte| *byte == 0)
        .ok_or(Error::Identity)?;
    let name = &tail[..end];
    if !valid_identity(name) {
        return Err(Error::Identity);
    }
    Ok(Zeroizing::new(name.to_vec()))
}

fn os_name(scratch: &mut [u8]) -> Result<Option<usize>, Error> {
    // SAFETY: geteuid has no pointer arguments. passwd and result are writable
    // local storage, and scratch is live writable storage for exactly its length.
    // No returned string pointer is dereferenced; only checked offsets are used.
    let uid = unsafe { libc::geteuid() };
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let code = unsafe {
        libc::getpwuid_r(
            uid,
            entry.as_mut_ptr(),
            scratch.as_mut_ptr().cast(),
            scratch.len(),
            &mut result,
        )
    };
    if code != 0 {
        return Err(Error::Lookup);
    }
    if result.is_null() {
        return Ok(None);
    }
    if result != entry.as_mut_ptr() {
        return Err(Error::Lookup);
    }
    // SAFETY: successful getpwuid_r returned the supplied passwd pointer.
    let entry = unsafe { entry.assume_init() };
    if entry.pw_uid != uid {
        return Err(Error::Identity);
    }
    let offset = (entry.pw_name as usize)
        .checked_sub(scratch.as_ptr() as usize)
        .filter(|offset| *offset < scratch.len())
        .ok_or(Error::Identity)?;
    Ok(Some(offset))
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness omits unit test bodies.
    use super::*;
    #[test]
    fn bounds_and_static_failure_categories() {
        for ceiling in [0, 257, usize::MAX] {
            assert_eq!(
                read_with(ceiling, |_| panic!("no OS access")).err(),
                Some(Error::Limit)
            );
        }
        assert_eq!(
            read_with(256, |_| Err(Error::Lookup)).err(),
            Some(Error::Lookup)
        );
        assert_eq!(read_with(256, |_| Ok(None)).err(), Some(Error::Missing));
        for offset in [SCRATCH_BYTES, usize::MAX] {
            assert_eq!(
                read_with(256, |_| Ok(Some(offset))).err(),
                Some(Error::Identity)
            );
        }
        assert_eq!(format!("{:?}", Error::Identity), "Identity");
    }
    #[test]
    fn identity_bounds_terminators_and_exact_owned_bytes() {
        for bad in [
            vec![],
            b"-fixture".to_vec(),
            b"fixture user".to_vec(),
            b"fixture|user".to_vec(),
            b"fixture\n".to_vec(),
            vec![0xff],
            vec![b'x'; 257],
        ] {
            assert_eq!(
                read_with(256, |scratch| {
                    scratch[..bad.len()].copy_from_slice(&bad);
                    Ok(Some(0))
                })
                .err(),
                Some(Error::Identity)
            );
        }
        assert_eq!(
            read_with(256, |scratch| {
                scratch.fill(b'x');
                Ok(Some(SCRATCH_BYTES - 1))
            })
            .err(),
            Some(Error::Identity)
        );
        let name = read_with(256, |scratch| {
            scratch[8..264].fill(b'x');
            Ok(Some(8))
        })
        .unwrap();
        assert_eq!(name.as_slice(), &[b'x'; 256]);
        assert_eq!(
            read_with(3, |scratch| {
                scratch[..4].copy_from_slice(b"four");
                Ok(Some(0))
            })
            .err(),
            Some(Error::Identity)
        );
        assert_eq!(
            read_with(4, |scratch| {
                scratch[..4].copy_from_slice(b"four");
                Ok(Some(0))
            })
            .unwrap()
            .as_slice(),
            b"four"
        );
    }
    #[test]
    fn effective_uid_composes_with_observation_policy() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "effective_user::tests::effective_user_child",
            ])
            .env_clear()
            .env("USER", "-invalid-fixture")
            .env("LOGNAME", "-invalid-fixture")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "effective-user child failed");
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("effective-user child timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    #[test]
    #[ignore = "trusted host lookup child with bounded parent supervision"]
    fn effective_user_child() {
        let plan = crate::observation_mapping::map("fixture", true, "suppress", lookup).unwrap();
        assert!(valid_identity(plan.user().as_bytes()));
        assert_eq!(plan.cluster(), "fixture");
        assert!(!plan.emit_discovered());
        // No USER/LOGNAME lookup: the selected passwd record is keyed by geteuid.
        let mut scratch = Zeroizing::new(vec![0; SCRATCH_BYTES]);
        let offset = os_name(&mut scratch).unwrap().unwrap();
        let tail = &scratch[offset..];
        let end = tail.iter().position(|byte| *byte == 0).unwrap();
        assert!(plan.user().as_bytes() == &tail[..end]);
    }
}
