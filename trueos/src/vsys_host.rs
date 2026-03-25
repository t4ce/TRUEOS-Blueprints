extern crate alloc;

use alloc::string::String;
use std::io::Write;

#[inline]
pub fn poll_once() {
    std::thread::yield_now();
}

#[inline]
pub fn write_stream(stream: u32, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }

    match stream {
        2 => {
            let mut stderr = std::io::stderr().lock();
            let _ = stderr.write_all(bytes);
            let _ = stderr.flush();
        }
        _ => {
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(bytes);
            let _ = stdout.flush();
        }
    }
}

#[inline]
pub fn write_log_stream(stream: u32, s: &str) {
    write_stream(stream, s.as_bytes());
}

#[inline]
pub fn log_info(s: &str) {
    write_log_stream(1, s);
}

#[inline]
pub fn log_error(s: &str) {
    write_log_stream(2, s);
}

#[inline]
pub fn log_info_with_args(prefix: &str, args: &[&str]) {
    log_with_args(1, prefix, args);
}

#[inline]
pub fn log_error_with_args(prefix: &str, args: &[&str]) {
    log_with_args(2, prefix, args);
}

fn log_with_args(stream: u32, prefix: &str, args: &[&str]) {
    let mut line = String::from(prefix);
    if args.is_empty() {
        line.push_str(" args=(none)\n");
    } else {
        line.push_str(" args=");
        for (idx, arg) in args.iter().enumerate() {
            if idx != 0 {
                line.push(' ');
            }
            line.push_str(arg);
        }
        line.push('\n');
    }

    write_log_stream(stream, line.as_str());
}