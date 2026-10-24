//! Real terminal input for bounded installed setup acceptance.
use std::{
    fs::File,
    io::Write,
    os::fd::FromRawFd,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
pub(super) fn run(mut command: Command, input: &[u8]) -> Output {
    assert!(
        input.len() < 4096,
        "fixture must fit a canonical terminal line"
    );
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: writable descriptors and null optional terminal settings/name storage.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    // SAFETY: openpty produced uniquely owned descriptors.
    let (mut master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    let mut child = command
        .stdin(Stdio::from(slave))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Early destination rejection may close terminal input before this write.
    let _ = master.write_all(input);
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        match child.try_wait().unwrap() {
            Some(_) => break,
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("setup terminal fixture exceeded its deadline");
            }
        }
    }
    child.wait_with_output().unwrap()
}
