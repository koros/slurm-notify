//! Private bounded ASCII reply reader; discards all server text.
use std::io::Read;

pub(super) struct Limits {
    pub(super) line_bytes: usize,
    pub(super) total_bytes: usize,
    pub(super) lines: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    InvalidLimit,
    Io,
    Truncated,
    Malformed,
    Limit,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Reply {
    pub(super) code: u16,
    pub(super) lines: usize,
    pub(super) bytes: usize,
}

// Read exactly one reply without consuming a later reply's bytes. Caller's
// transport/parent deadline must bound blocking Read calls; byte limits do not.
pub(super) fn read_reply(mut input: impl Read, limits: Limits) -> Result<Reply, Error> {
    if limits.line_bytes < 5 || limits.total_bytes < 5 || limits.lines == 0 {
        return Err(Error::InvalidLimit);
    }
    let mut total = 0;
    let mut expected = None;
    for number in 1..=limits.lines {
        let mut line = Vec::new();
        loop {
            if line.len() == limits.line_bytes || total == limits.total_bytes {
                return Err(Error::Limit);
            }
            let mut byte = [0];
            match input.read(&mut byte) {
                Ok(0) => return Err(Error::Truncated),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(Error::Io),
            }
            total += 1;
            line.push(byte[0]);
            if byte[0] == b'\n' {
                break;
            }
        }
        if !line.ends_with(b"\r\n") {
            return Err(Error::Malformed);
        }
        let body = &line[..line.len() - 2];
        if body.len() < 3
            || !(b'2'..=b'5').contains(&body[0])
            || !(b'0'..=b'5').contains(&body[1])
            || !body[2].is_ascii_digit()
        {
            return Err(Error::Malformed);
        }
        let code = u16::from(body[0] - b'0') * 100
            + u16::from(body[1] - b'0') * 10
            + u16::from(body[2] - b'0');
        if expected.is_some_and(|previous| previous != code) {
            return Err(Error::Malformed);
        }
        expected = Some(code);
        let continuation = match body.get(3) {
            None => false,
            Some(b' ') => false,
            Some(b'-') => true,
            _ => return Err(Error::Malformed),
        };
        if body
            .iter()
            .skip(4)
            .any(|byte| !matches!(byte, b'\t' | 32..=126))
        {
            return Err(Error::Malformed);
        }
        if !continuation {
            return Ok(Reply {
                code,
                lines: number,
                bytes: total,
            });
        }
    }
    Err(Error::Limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn limits() -> Limits {
        Limits {
            line_bytes: 64,
            total_bytes: 128,
            lines: 4,
        }
    }

    #[test]
    fn multiline_reply_preserves_code_and_leaves_next_reply_unread() {
        let mut input =
            std::io::Cursor::new(b"250-first\r\n250-second\r\n250 final\r\n220 next\r\n");
        let reply = read_reply(&mut input, limits()).unwrap();
        assert_eq!((reply.code, reply.lines), (250, 3));
        assert_eq!(input.position() as usize, reply.bytes);
        assert_eq!(read_reply(&mut input, limits()).unwrap().code, 220);
        assert_eq!(read_reply(&b"250\r\n"[..], limits()).unwrap().code, 250);
        assert!(!format!("{reply:?}").contains("first"));
    }

    #[test]
    fn malformed_and_truncated_replies_never_expose_server_text() {
        for bytes in [
            &b"250 bad\n"[..],
            &b"999 bad\r\n"[..],
            &b"260 bad\r\n"[..],
            &b"250xsecret\r\n"[..],
            &b"250 secret\0\r\n"[..],
            &b"250-hello\r\n550 changed\r\n"[..],
            &b"250 bad\rtext\r\n"[..],
            &b"250 non-ascii\xff\r\n"[..],
        ] {
            assert_eq!(read_reply(bytes, limits()), Err(Error::Malformed));
        }
        for bytes in [&b""[..], &b"250 partial\r"[..], &b"250-continue\r\n"[..]] {
            assert_eq!(read_reply(bytes, limits()), Err(Error::Truncated));
        }
    }

    #[test]
    fn exact_byte_boundaries_work_and_line_total_and_count_limits_are_enforced() {
        assert_eq!(
            read_reply(
                &b"250\r\n"[..],
                Limits {
                    line_bytes: 5,
                    total_bytes: 5,
                    lines: 1
                }
            )
            .unwrap()
            .bytes,
            5
        );
        for (bytes, limits) in [
            (
                &b"250 text\r\n"[..],
                Limits {
                    line_bytes: 5,
                    total_bytes: 50,
                    lines: 1,
                },
            ),
            (
                &b"250-one\r\n250 two\r\n"[..],
                Limits {
                    line_bytes: 50,
                    total_bytes: 10,
                    lines: 2,
                },
            ),
            (
                &b"250-one\r\n250 two\r\n"[..],
                Limits {
                    line_bytes: 50,
                    total_bytes: 50,
                    lines: 1,
                },
            ),
        ] {
            assert_eq!(read_reply(bytes, limits), Err(Error::Limit));
        }
        for limits in [
            Limits {
                line_bytes: 0,
                total_bytes: 50,
                lines: 1,
            },
            Limits {
                line_bytes: 50,
                total_bytes: 0,
                lines: 1,
            },
            Limits {
                line_bytes: 50,
                total_bytes: 50,
                lines: 0,
            },
        ] {
            assert_eq!(
                read_reply(&b"250\r\n"[..], limits),
                Err(Error::InvalidLimit)
            );
        }
    }

    #[test]
    fn io_errors_are_static_and_interrupted_reads_are_retried() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "private-server-fixture",
                ))
            }
        }
        assert_eq!(read_reply(Broken, limits()), Err(Error::Io));
        struct Interrupted {
            once: bool,
            data: std::io::Cursor<&'static [u8]>,
        }
        impl Read for Interrupted {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                if !self.once {
                    self.once = true;
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                self.data.read(bytes)
            }
        }
        assert_eq!(
            read_reply(
                Interrupted {
                    once: false,
                    data: std::io::Cursor::new(b"250\r\n")
                },
                limits()
            )
            .unwrap()
            .code,
            250
        );
    }
}
