#![cfg_attr(not(feature = "host-std"), no_std)]

extern crate alloc;

use alloc::{borrow::Cow, format, string::String};

use trueos::{portal, venv, vfetch, vfs, vshell};

const DEFAULT_FETCH_TIMEOUT_MS: u64 = 30_000;

fn app_main(args: &[&str]) {
    print_line("localcoder_bp: TRUEOS portal scaffold");
    print_line(
        "localcoder_bp: commands: help | args | env [KEY] | read <PATH> | write <PATH> <TEXT> | fetch <URL> | history [START] [COUNT]",
    );

    let Some(cmd) = args.get(1).copied() else {
        print_args(args);
        print_pwd_hint();
        return;
    };

    match cmd {
        "help" => print_help(),
        "args" => print_args(args),
        "env" => print_env(args),
        "read" => read_command(args),
        "write" => write_command(args),
        "fetch" => fetch_command(args),
        "history" => history_command(args),
        _ => {
            print_line(&format!("unknown command: {cmd}"));
            print_line("try: help");
        }
    }
}

fn print_help() {
    print_line("help: show available commands");
    print_line("args: show launch argv");
    print_line("env [KEY]: show env vars or a single variable");
    print_line("read <PATH>: read bytes through TRUEOS FS");
    print_line("write <PATH> <TEXT>: write bytes through TRUEOS FS");
    print_line("fetch <URL>: fetch bytes through TRUEOS network");
    print_line("history [START] [COUNT]: dump shell history text");
}

fn print_args(args: &[&str]) {
    print_line(&format!("argc={}", args.len()));
    for (index, arg) in args.iter().enumerate() {
        print_line(&format!("argv[{index}] = {arg}"));
    }
}

fn print_pwd_hint() {
    print_env_var_or_unset("PWD");
    print_env_var_or_unset("TRUEOS_APP_ARCHIVE");
}

fn print_env(args: &[&str]) {
    if let Some(key) = args.get(2).copied() {
        print_env_var_or_unset(key);
        return;
    }
    print_pwd_hint();
}

fn print_env_var_or_unset(key: &str) {
    match venv::var(key) {
        Ok(value) => print_line(&format!("{key}={value}")),
        Err(_) => print_line(&format!("{key}=<unset>")),
    }
}

fn read_command(args: &[&str]) {
    let Some(path) = args.get(2).copied() else {
        print_line("usage: read <PATH>");
        return;
    };

    match vfs::read_file(path.as_bytes()) {
        Ok(bytes) => {
            print_line(&format!("read {} bytes from {path}", bytes.len()));
            print_multiline_lossy(&bytes);
        }
        Err(rc) => print_line(&format!("read failed rc={rc}")),
    }
}

fn write_command(args: &[&str]) {
    let Some(path) = args.get(2).copied() else {
        print_line("usage: write <PATH> <TEXT>");
        return;
    };
    let Some(text) = args.get(3).copied() else {
        print_line("usage: write <PATH> <TEXT>");
        return;
    };

    let text_bytes = text.as_bytes();
    let handle = match vfs::write_begin(path.as_bytes(), text_bytes.len() as u64) {
        Ok(handle) => handle,
        Err(rc) => {
            print_line(&format!("write failed rc={rc}"));
            return;
        }
    };

    if let Err(rc) = vfs::write_chunk(handle, text_bytes) {
        let _ = vfs::write_abort(handle);
        print_line(&format!("write failed rc={rc}"));
        return;
    }

    if let Err(rc) = vfs::write_finish(handle) {
        let _ = vfs::write_abort(handle);
        print_line(&format!("write failed rc={rc}"));
        return;
    }

    print_line(&format!("write ok path={path} bytes={}", text_bytes.len()));
}

fn fetch_command(args: &[&str]) {
    let Some(url) = args.get(2).copied() else {
        print_line("usage: fetch <URL>");
        return;
    };

    let op_id = match vfetch::fetch_bytes(url.as_bytes()) {
        Ok(op_id) => op_id,
        Err(rc) => {
            print_line(&format!("fetch failed rc={rc}"));
            return;
        }
    };

    if let Err(rc) = wait_for_fetch(op_id) {
        let _ = vfetch::fetch_bytes_discard(op_id);
        print_line(&format!("fetch wait failed rc={rc}"));
        return;
    }

    match vfetch::fetch_bytes_read(op_id) {
        Ok(bytes) => {
            let _ = vfetch::fetch_bytes_discard(op_id);
            print_line(&format!("fetch {} bytes from {url}", bytes.len()));
            print_multiline_lossy(&bytes);
        }
        Err(rc) => {
            let _ = vfetch::fetch_bytes_discard(op_id);
            print_line(&format!("fetch read failed rc={rc}"));
        }
    }
}

fn wait_for_fetch(op_id: u32) -> Result<(), i32> {
    let rc = vfetch::fetch_bytes_wait(op_id, DEFAULT_FETCH_TIMEOUT_MS);
    if rc == 0 {
        Ok(())
    } else {
        Err(rc)
    }
}

fn history_command(args: &[&str]) {
    let total = vshell::shell1_history_total_lines();
    let start = args
        .get(2)
        .copied()
        .and_then(parse_usize_ascii)
        .unwrap_or(0);
    let count = args
        .get(3)
        .copied()
        .and_then(parse_usize_ascii)
        .unwrap_or(total);

    match vshell::shell1_history_text_since(start, count) {
        Some(text) => {
            print_line(&format!("history total={total} start={start} count={count}"));
            print_multiline(&text);
        }
        None => print_line("history failed rc=-1"),
    }
}

fn print_multiline_lossy(bytes: &[u8]) {
    if bytes.is_empty() {
        print_line("<empty>");
        return;
    }

    let text: Cow<'_, str> = String::from_utf8_lossy(bytes);
    print_multiline(&text);
}

fn print_multiline(text: &str) {
    if text.is_empty() {
        print_line("<empty>");
        return;
    }

    for line in text.lines() {
        print_line(line.trim_end_matches('\r'));
    }
}

fn parse_usize_ascii(text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }

    let mut value = 0usize;
    for byte in text.bytes() {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?;
        value = value.checked_add((byte - b'0') as usize)?;
    }
    Some(value)
}

fn print_line(line: &str) {
    vshell::shell2_print_line(line.as_bytes());
}

portal!(app_main);
