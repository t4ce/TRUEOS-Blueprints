use core::fmt;

use crate::vcabi;

#[inline]
pub fn attached_write(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { vcabi::trueos_cabi_shell_attached_write(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn attached_write_str(text: &str) -> usize {
    attached_write(text.as_bytes())
}

#[inline]
pub fn attached_write_fmt(args: fmt::Arguments<'_>) -> usize {
    struct AttachedWriter(usize);

    impl fmt::Write for AttachedWriter {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.0 += attached_write_str(s);
            Ok(())
        }
    }

    let mut writer = AttachedWriter(0);
    let _ = fmt::Write::write_fmt(&mut writer, args);
    writer.0
}

#[inline]
pub fn attached_read_byte() -> Option<u8> {
    let value = unsafe { vcabi::trueos_cabi_shell_attached_read_byte() };
    if (0..=255).contains(&value) {
        Some(value as u8)
    } else {
        None
    }
}
