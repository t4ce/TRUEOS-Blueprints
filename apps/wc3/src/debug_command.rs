//! Bounded, read-only commands for the WC3 Blueprint command channel.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    State,
    Registers {
        pid: u32,
        tid: u32,
    },
    Memory {
        pid: u32,
        address: u32,
        bytes: usize,
    },
    Stack {
        pid: u32,
        tid: u32,
        words: usize,
    },
    Object {
        pid: u32,
        handle: u32,
    },
}
fn number(value: &str) -> Result<u32, &'static str> {
    if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).map_err(|_| "invalid hexadecimal number")
    } else {
        value.parse().map_err(|_| "invalid decimal number")
    }
}
pub fn parse(line: &str) -> Result<Command, &'static str> {
    let words: Vec<_> = line.split_whitespace().collect();
    Ok(match words.as_slice() {
        ["debug"] | ["debug", "help"] => Command::Help,
        ["debug", "state"] => Command::State,
        ["debug", "regs", pid, tid] => Command::Registers {
            pid: number(pid)?,
            tid: number(tid)?,
        },
        ["debug", "object", pid, handle] => Command::Object {
            pid: number(pid)?,
            handle: number(handle)?,
        },
        ["debug", "mem", pid, address, bytes] => {
            let address = number(address)?;
            let bytes = number(bytes)?;
            if bytes == 0 || bytes > 256 || address.checked_add(bytes).is_none() {
                return Err("memory range must be 1..256 bytes without address overflow");
            }
            Command::Memory {
                pid: number(pid)?,
                address,
                bytes: bytes as usize,
            }
        }
        ["debug", "stack", pid, tid, words] => {
            let words = number(words)?;
            if words == 0 || words > 64 {
                return Err("stack count must be 1..64 dwords");
            }
            Command::Stack {
                pid: number(pid)?,
                tid: number(tid)?,
                words: words as usize,
            }
        }
        _ => return Err("unknown command; use debug help"),
    })
}

/// A too-long line is rejected in full; its suffix must never become a command.
#[derive(Default)]
pub struct Lines {
    bytes: Vec<u8>,
    overflow: bool,
}
impl Lines {
    pub fn push(&mut self, byte: u8) -> Option<Result<Command, &'static str>> {
        if byte == b'\r' || byte == b'\n' {
            let overflow = core::mem::take(&mut self.overflow);
            let bytes = core::mem::take(&mut self.bytes);
            if overflow {
                return Some(Err("debug command exceeds 160 bytes"));
            }
            if bytes.is_empty() {
                return None;
            }
            return Some(
                core::str::from_utf8(&bytes)
                    .map_err(|_| "invalid command text")
                    .and_then(parse),
            );
        }
        if !self.overflow {
            if self.bytes.len() == 160 {
                self.overflow = true;
                self.bytes.clear();
            } else {
                self.bytes.push(byte);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_and_no_mutation_commands() {
        assert_eq!(
            parse("debug mem 2 0x444480 256"),
            Ok(Command::Memory {
                pid: 2,
                address: 0x444480,
                bytes: 256
            })
        );
        for s in [
            "debug mem 2 0xffffffff 2",
            "debug mem 2 0 257",
            "debug mem 2 0 0",
            "debug stack 2 3 65",
            "debug set 2 0 1",
            "debug state extra",
        ] {
            assert!(parse(s).is_err(), "{s}");
        }
    }
    #[test]
    fn fragmented_lines_crlf_and_overflow_recovery() {
        let mut lines = Lines::default();
        for b in b"debug state" {
            assert!(lines.push(*b).is_none());
        }
        assert_eq!(lines.push(b'\r'), Some(Ok(Command::State)));
        assert_eq!(lines.push(b'\n'), None);
        for _ in 0..161 {
            lines.push(b'x');
        }
        for b in b"debug state" {
            lines.push(*b);
        }
        assert!(lines.push(b'\n').unwrap().is_err());
        for b in b"debug help" {
            lines.push(*b);
        }
        assert_eq!(lines.push(b'\n'), Some(Ok(Command::Help)));
    }
}
