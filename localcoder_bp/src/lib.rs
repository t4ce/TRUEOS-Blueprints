#![no_std]
#![allow(unsafe_op_in_unsafe_fn)]

use core::ffi::c_char;
use core::panic::PanicInfo;
use core::{ptr, slice};

use trueos_v::vcabi;

const DEFAULT_FETCH_TIMEOUT_MS: u64 = 30_000;

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    print_line(b"localcoder_bp: panic");
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn main(argc: usize, argv: *const *const c_char) {
    print_line(b"localcoder_bp: TRUEOS portal scaffold");
    print_line(
        b"localcoder_bp: commands: help | args | env [KEY] | read <PATH> | write <PATH> <TEXT> | fetch <URL> | history [START] [COUNT]",
    );

    let args = Args { argc, argv };
    let Some(cmd) = args.get(1) else {
        print_args(args);
        print_pwd_hint();
        return;
    };

    if bytes_eq(cmd, b"help") {
        print_line(b"help: show available commands");
        print_line(b"args: show launch argv");
        print_line(b"env [KEY]: show env vars or a single variable");
        print_line(b"read <PATH>: read bytes through TRUEOS FS CABI");
        print_line(b"write <PATH> <TEXT>: write bytes through TRUEOS FS CABI");
        print_line(b"fetch <URL>: fetch bytes through TRUEOS net CABI");
        print_line(b"history [START] [COUNT]: dump shell history text");
        return;
    }
    if bytes_eq(cmd, b"args") {
        print_args(args);
        return;
    }
    if bytes_eq(cmd, b"env") {
        print_env(args);
        return;
    }
    if bytes_eq(cmd, b"read") {
        read_command(args);
        return;
    }
    if bytes_eq(cmd, b"write") {
        write_command(args);
        return;
    }
    if bytes_eq(cmd, b"fetch") {
        fetch_command(args);
        return;
    }
    if bytes_eq(cmd, b"history") {
        history_command(args);
        return;
    }

    print_prefixed_line(b"unknown command: ", cmd);
    print_line(b"try: help");
}

#[derive(Copy, Clone)]
struct Args {
    argc: usize,
    argv: *const *const c_char,
}

impl Args {
    fn get(self, index: usize) -> Option<&'static [u8]> {
        if index >= self.argc || self.argv.is_null() {
            return None;
        }
        let ptr = unsafe { *self.argv.add(index) } as *const u8;
        if ptr.is_null() {
            return None;
        }
        Some(c_str_bytes(ptr))
    }
}

fn c_str_bytes(ptr: *const u8) -> &'static [u8] {
    let mut len = 0usize;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        slice::from_raw_parts(ptr, len)
    }
}

fn print_args(args: Args) {
    let mut line = LineBuf::new();
    line.push_bytes(b"argc=");
    line.push_usize(args.argc);
    print_line(line.as_bytes());

    let mut index = 0usize;
    while index < args.argc {
        let mut item = LineBuf::new();
        item.push_bytes(b"argv[");
        item.push_usize(index);
        item.push_bytes(b"] = ");
        if let Some(arg) = args.get(index) {
            item.push_bytes(arg);
        }
        print_line(item.as_bytes());
        index += 1;
    }
}

fn print_pwd_hint() {
    print_env_var_or_unset(b"PWD");
    print_env_var_or_unset(b"TRUEOS_APP_ARCHIVE");
}

fn print_env(args: Args) {
    if let Some(key) = args.get(2) {
        print_env_var_or_unset(key);
        return;
    }
    print_pwd_hint();
}

fn print_env_var_or_unset(key: &[u8]) {
    let len = unsafe { vcabi::trueos_cabi_env_var(key.as_ptr(), key.len(), ptr::null_mut(), 0) };
    if len < 0 {
        let mut line = LineBuf::new();
        line.push_bytes(key);
        line.push_bytes(b"=<unset>");
        print_line(line.as_bytes());
        return;
    }

    with_alloc_buffer(len as usize, |buf| unsafe {
        let got = vcabi::trueos_cabi_env_var(key.as_ptr(), key.len(), buf, len as usize);
        if got < 0 {
            let mut line = LineBuf::new();
            line.push_bytes(key);
            line.push_bytes(b"=<error>");
            print_line(line.as_bytes());
            return;
        }
        let value = slice::from_raw_parts(buf, got as usize);
        let mut line = LineBuf::new();
        line.push_bytes(key);
        line.push_bytes(b"=");
        line.push_bytes(value);
        print_line(line.as_bytes());
    });
}

fn read_command(args: Args) {
    let Some(path) = args.get(2) else {
        print_line(b"usage: read <PATH>");
        return;
    };

    let len =
        unsafe { vcabi::trueos_cabi_fs_read_file(path.as_ptr(), path.len(), ptr::null_mut(), 0) };
    if len < 0 {
        print_rc_line(b"read failed rc=", len);
        return;
    }

    with_alloc_buffer(len as usize, |buf| unsafe {
        let got = vcabi::trueos_cabi_fs_read_file(path.as_ptr(), path.len(), buf, len as usize);
        if got < 0 {
            print_rc_line(b"read failed rc=", got);
            return;
        }
        let mut line = LineBuf::new();
        line.push_bytes(b"read ");
        line.push_usize(got as usize);
        line.push_bytes(b" bytes from ");
        line.push_bytes(path);
        print_line(line.as_bytes());
        print_multiline(slice::from_raw_parts(buf, got as usize));
    });
}

fn write_command(args: Args) {
    let Some(path) = args.get(2) else {
        print_line(b"usage: write <PATH> <TEXT>");
        return;
    };
    let Some(text) = args.get(3) else {
        print_line(b"usage: write <PATH> <TEXT>");
        return;
    };

    let mut handle = 0u32;
    let begin = unsafe {
        vcabi::trueos_cabi_fs_write_begin(path.as_ptr(), path.len(), text.len() as u64, &mut handle)
    };
    if begin != 0 {
        print_rc_line_i32(b"write failed rc=", begin);
        return;
    }

    let chunk = unsafe { vcabi::trueos_cabi_fs_write_chunk(handle, text.as_ptr(), text.len()) };
    if chunk != 0 {
        unsafe {
            let _ = vcabi::trueos_cabi_fs_write_abort(handle);
        }
        print_rc_line_i32(b"write failed rc=", chunk);
        return;
    }

    let finish = unsafe { vcabi::trueos_cabi_fs_write_finish(handle) };
    if finish != 0 {
        unsafe {
            let _ = vcabi::trueos_cabi_fs_write_abort(handle);
        }
        print_rc_line_i32(b"write failed rc=", finish);
        return;
    }

    let mut line = LineBuf::new();
    line.push_bytes(b"write ok path=");
    line.push_bytes(path);
    line.push_bytes(b" bytes=");
    line.push_usize(text.len());
    print_line(line.as_bytes());
}

fn fetch_command(args: Args) {
    let Some(url) = args.get(2) else {
        print_line(b"usage: fetch <URL>");
        return;
    };

    let op_id = unsafe { vcabi::trueos_cabi_net_fetch_bytes_start(url.as_ptr(), url.len()) };
    if op_id == 0 {
        print_line(b"fetch failed rc=start");
        return;
    }

    let wait_rc = unsafe { vcabi::trueos_cabi_net_fetch_bytes_wait(op_id, DEFAULT_FETCH_TIMEOUT_MS) };
    if wait_rc != 0 {
        unsafe {
            let _ = vcabi::trueos_cabi_net_fetch_bytes_discard(op_id);
        }
        print_rc_line_i32(b"fetch wait failed rc=", wait_rc);
        return;
    }

    let len = unsafe { vcabi::trueos_cabi_net_fetch_bytes_result_len(op_id) };
    if len < 0 {
        unsafe {
            let _ = vcabi::trueos_cabi_net_fetch_bytes_discard(op_id);
        }
        print_rc_line(b"fetch result failed rc=", len);
        return;
    }

    with_alloc_buffer(len as usize, |buf| unsafe {
        let got = vcabi::trueos_cabi_net_fetch_bytes_read(op_id, buf, len as usize);
        let _ = vcabi::trueos_cabi_net_fetch_bytes_discard(op_id);
        if got < 0 {
            print_rc_line(b"fetch read failed rc=", got);
            return;
        }
        let mut line = LineBuf::new();
        line.push_bytes(b"fetch ");
        line.push_usize(got as usize);
        line.push_bytes(b" bytes from ");
        line.push_bytes(url);
        print_line(line.as_bytes());
        print_multiline(slice::from_raw_parts(buf, got as usize));
    });
}

fn history_command(args: Args) {
    let total = unsafe { vcabi::trueos_cabi_shell_history_lines_all() };
    let start = args
        .get(2)
        .and_then(parse_usize_ascii)
        .unwrap_or(0);
    let count = args
        .get(3)
        .and_then(parse_usize_ascii)
        .unwrap_or(total);

    let len =
        unsafe { vcabi::trueos_cabi_shell_history_lines(start, count, ptr::null_mut(), 0) };
    if len < 0 {
        print_rc_line(b"history failed rc=", len);
        return;
    }

    with_alloc_buffer(len as usize, |buf| unsafe {
        let got = vcabi::trueos_cabi_shell_history_lines(start, count, buf, len as usize);
        if got < 0 {
            print_rc_line(b"history failed rc=", got);
            return;
        }

        let mut line = LineBuf::new();
        line.push_bytes(b"history total=");
        line.push_usize(total);
        line.push_bytes(b" start=");
        line.push_usize(start);
        line.push_bytes(b" count=");
        line.push_usize(count);
        print_line(line.as_bytes());
        print_multiline(slice::from_raw_parts(buf, got as usize));
    });
}

fn print_multiline(bytes: &[u8]) {
    if bytes.is_empty() {
        print_line(b"<empty>");
        return;
    }

    let mut start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            print_line(trim_cr(subslice(bytes, start, index)));
            start = index + 1;
        }
        index += 1;
    }
    if start < bytes.len() {
        print_line(trim_cr(tail(bytes, start)));
    }
}

fn trim_cr(bytes: &[u8]) -> &[u8] {
    if bytes.last().copied() == Some(b'\r') {
        unsafe { slice::from_raw_parts(bytes.as_ptr(), bytes.len() - 1) }
    } else {
        bytes
    }
}

fn subslice(bytes: &[u8], start: usize, end: usize) -> &[u8] {
    unsafe { slice::from_raw_parts(bytes.as_ptr().add(start), end - start) }
}

fn tail(bytes: &[u8], start: usize) -> &[u8] {
    unsafe { slice::from_raw_parts(bytes.as_ptr().add(start), bytes.len() - start) }
}

fn parse_usize_ascii(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() {
        return None;
    }
    let mut value = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        let digit = bytes[index];
        if !digit.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?;
        value = value.checked_add((digit - b'0') as usize)?;
        index += 1;
    }
    Some(value)
}

fn bytes_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0usize;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

fn print_prefixed_line(prefix: &[u8], value: &[u8]) {
    let mut line = LineBuf::new();
    line.push_bytes(prefix);
    line.push_bytes(value);
    print_line(line.as_bytes());
}

fn print_rc_line(prefix: &[u8], value: isize) {
    let mut line = LineBuf::new();
    line.push_bytes(prefix);
    line.push_isize(value);
    print_line(line.as_bytes());
}

fn print_rc_line_i32(prefix: &[u8], value: i32) {
    let mut line = LineBuf::new();
    line.push_bytes(prefix);
    line.push_isize(value as isize);
    print_line(line.as_bytes());
}

fn print_line(bytes: &[u8]) {
    unsafe {
        let _ = vcabi::trueos_cabi_shell2_print_line(bytes.as_ptr(), bytes.len());
    }
}

fn with_alloc_buffer<F>(len: usize, f: F)
where
    F: FnOnce(*mut u8),
{
    if len == 0 {
        f(ptr::null_mut());
        return;
    }

    unsafe {
        let buf = vcabi::trueos_cabi_alloc(len);
        if buf.is_null() {
            print_line(b"localcoder_bp: alloc failed");
            return;
        }
        f(buf);
        vcabi::trueos_cabi_free(buf);
    }
}

struct LineBuf {
    bytes: [u8; 512],
    len: usize,
}

impl LineBuf {
    fn new() -> Self {
        Self {
            bytes: [0; 512],
            len: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.bytes.as_ptr(), self.len) }
    }

    fn push_bytes(&mut self, src: &[u8]) {
        let mut index = 0usize;
        while index < src.len() && self.len < self.bytes.len() {
            self.bytes[self.len] = src[index];
            self.len += 1;
            index += 1;
        }
    }

    fn push_byte(&mut self, byte: u8) {
        if self.len < self.bytes.len() {
            self.bytes[self.len] = byte;
            self.len += 1;
        }
    }

    fn push_usize(&mut self, value: usize) {
        self.push_u128(value as u128);
    }

    fn push_isize(&mut self, value: isize) {
        if value < 0 {
            self.push_bytes(b"-");
            self.push_u128(value.unsigned_abs() as u128);
        } else {
            self.push_u128(value as u128);
        }
    }

    fn push_u128(&mut self, mut value: u128) {
        if value == 0 {
            self.push_bytes(b"0");
            return;
        }

        let mut tmp = [0u8; 39];
        let mut used = 0usize;
        while value != 0 && used < tmp.len() {
            tmp[used] = b'0' + (value % 10) as u8;
            value /= 10;
            used += 1;
        }
        while used > 0 {
            used -= 1;
            self.push_byte(tmp[used]);
        }
    }
}
