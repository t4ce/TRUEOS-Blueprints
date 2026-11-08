pub(crate) use crate::common_term::*;
use crate::{kb::Key, term::Term};
use std::{fmt::Display, io};
pub(crate) const DEFAULT_WIDTH: u16 = 80;
pub(crate) fn is_a_terminal(out: &Term) -> bool {
    out.native_is_terminal()
}
pub(crate) fn is_a_color_terminal(out: &Term) -> bool {
    is_a_terminal(out)
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").is_ok_and(|term| term != "dumb")
}
pub(crate) fn terminal_size(_: &Term) -> Option<(u16, u16)> {
    None
}
pub(crate) fn read_secure() -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native console secure input requires a terminal lease",
    ))
}
pub(crate) fn read_single_key(_: bool) -> io::Result<Key> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native console key input requires a terminal lease",
    ))
}
pub(crate) fn wants_emoji() -> bool {
    std::env::var("LANG").is_ok_and(|lang| {
        let lang = lang.to_ascii_lowercase();
        lang.ends_with("utf-8") || lang.ends_with("utf8")
    })
}
pub(crate) fn set_title<T: Display>(title: T) {
    print!("\x1b]0;{}\x07", title);
}
