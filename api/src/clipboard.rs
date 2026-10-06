//! Opt-in typed clipboard. Paste takes only OS-authorized delivery events.
use alloc::string::String;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Kind {
    Text = 1,
    Password = 2,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error(pub i32);
impl Error {
    pub fn message(self) -> &'static str {
        match self.0 {
            -12 => "TRUEOS Secure Copyservice needs authenticated user configuration",
            -14 => "Clipboard content does not match this field",
            -1 => "Clipboard text is too long",
            -4 => "Clipboard is busy; try again",
            _ => "TRUEOS clipboard is unavailable",
        }
    }
}
fn command(
    window: u32,
    action: u32,
    kind: u32,
    input: &[u8],
    output: &mut [u8],
) -> Result<usize, Error> {
    let rc = unsafe {
        v::bp_abi::trueos_cabi_clipboard_command_v1(
            window,
            action,
            kind,
            input.as_ptr(),
            input.len(),
            output.as_mut_ptr(),
            output.len(),
        )
    };
    if rc < 0 {
        Err(Error(rc))
    } else {
        Ok(rc as usize)
    }
}
pub fn copy(window: u32, kind: Kind, text: &str) -> Result<(), Error> {
    if text.len() > 512 {
        return Err(Error(-1));
    }
    command(window, 1, kind as u32, text.as_bytes(), &mut []).map(|_| ())
}
pub fn focus(window: u32, kind: Option<Kind>) -> Result<(), Error> {
    command(window, 2, kind.map_or(0, |k| k as u32), &[], &mut []).map(|_| ())
}
pub fn take_paste(window: u32, kind: Kind) -> Result<Option<String>, Error> {
    let mut bytes = [0u8; 512];
    let result = command(window, 3, kind as u32, &[], &mut bytes).and_then(|len| {
        if len == 0 {
            Ok(None)
        } else if len <= bytes.len() {
            core::str::from_utf8(&bytes[..len])
                .map(|s| Some(String::from(s)))
                .map_err(|_| Error(-1))
        } else {
            Err(Error(-1))
        }
    });
    // Wipe the temporary delivery buffer, including rejected/invalid events.
    for byte in &mut bytes {
        unsafe {
            core::ptr::write_volatile(byte, 0);
        }
    }
    result
}
