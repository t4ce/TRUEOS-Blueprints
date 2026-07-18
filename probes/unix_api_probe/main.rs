use core::ffi::{c_int, c_short, c_ulong, c_void};

use trueos::{logl, logl::level, platform};

const STDIN_FILENO: c_int = 0;
const STDOUT_FILENO: c_int = 1;

const AF_UNIX: c_int = 1;
const SOCK_STREAM: c_int = 1;

const F_GETFL: c_int = 3;
const F_SETFL: c_int = 4;
const O_NONBLOCK: c_int = 0o4000;

const POLLIN: c_short = 0x0001;
const POLLOUT: c_short = 0x0004;

const TCSANOW: c_int = 0;
const TCGETS: c_ulong = 0x5401;
const TIOCGWINSZ: c_ulong = 0x5413;
const FIONREAD: c_ulong = 0x541b;

#[repr(C)]
#[derive(Clone, Copy)]
struct PollFd {
    fd: c_int,
    events: c_short,
    revents: c_short,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

unsafe extern "C" {
    fn __errno_location() -> *mut c_int;
    fn isatty(fd: c_int) -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, argp: *mut c_void) -> c_int;
    fn tcgetattr(fd: c_int, termios: *mut c_void) -> c_int;
    fn tcsetattr(fd: c_int, optional_actions: c_int, termios: *const c_void) -> c_int;
    fn pipe(fds: *mut c_int) -> c_int;
    fn socketpair(domain: c_int, socket_type: c_int, protocol: c_int, sv: *mut c_int) -> c_int;
    fn fcntl(fd: c_int, cmd: c_int, arg: c_int) -> c_int;
    fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    fn close(fd: c_int) -> c_int;
}

fn errno() -> c_int {
    unsafe { *__errno_location() }
}

fn clear_errno() {
    unsafe {
        *__errno_location() = 0;
    }
}

fn log_stage(stage: &str) {
    logl::log(level::INFO, format_args!("unix-api-probe: stage {}", stage));
}

fn log_rc_zero(stage: &str, rc: c_int) -> bool {
    if rc == 0 {
        logl::log(
            level::INFO,
            format_args!(
                "unix-api-probe: success {} rc={} errno={}",
                stage,
                rc,
                errno()
            ),
        );
        true
    } else {
        logl::log(
            level::ERROR,
            format_args!(
                "unix-api-probe: failed {} rc={} errno={}",
                stage,
                rc,
                errno()
            ),
        );
        false
    }
}

fn log_rc_nonnegative(stage: &str, rc: c_int) -> bool {
    if rc >= 0 {
        logl::log(
            level::INFO,
            format_args!(
                "unix-api-probe: success {} rc={} errno={}",
                stage,
                rc,
                errno()
            ),
        );
        true
    } else {
        logl::log(
            level::ERROR,
            format_args!(
                "unix-api-probe: failed {} rc={} errno={}",
                stage,
                rc,
                errno()
            ),
        );
        false
    }
}

fn log_fd(stage: &str, fd: c_int) -> bool {
    if fd >= 0 {
        logl::log(
            level::INFO,
            format_args!("unix-api-probe: success {} fd={}", stage, fd),
        );
        true
    } else {
        logl::log(
            level::ERROR,
            format_args!(
                "unix-api-probe: failed {} fd={} errno={}",
                stage,
                fd,
                errno()
            ),
        );
        false
    }
}

fn log_io(stage: &str, got: isize, expected: usize) -> bool {
    if got == expected as isize {
        logl::log(
            level::INFO,
            format_args!(
                "unix-api-probe: success {} bytes={} expected={}",
                stage, got, expected
            ),
        );
        true
    } else {
        logl::log(
            level::ERROR,
            format_args!(
                "unix-api-probe: failed {} bytes={} expected={} errno={}",
                stage,
                got,
                expected,
                errno()
            ),
        );
        false
    }
}

fn pipe_fds_valid(stage: &str, fds: [c_int; 2]) -> bool {
    let valid = fds[0] >= 0 && fds[1] >= 0;
    logl::log(
        if valid { level::INFO } else { level::ERROR },
        format_args!(
            "unix-api-probe: {} fds=[{},{}] valid={} errno={}",
            stage,
            fds[0],
            fds[1],
            valid,
            errno()
        ),
    );
    valid
}

fn probe_tty() -> bool {
    let mut ok = true;

    log_stage("isatty.stdin");
    clear_errno();
    ok &= log_rc_nonnegative("isatty.stdin", unsafe { isatty(STDIN_FILENO) });

    log_stage("isatty.stdout");
    clear_errno();
    ok &= log_rc_nonnegative("isatty.stdout", unsafe { isatty(STDOUT_FILENO) });

    let mut winsize = Winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    log_stage("ioctl.TIOCGWINSZ.stdout");
    clear_errno();
    let winsize_ok = log_rc_zero("ioctl.TIOCGWINSZ.stdout", unsafe {
        ioctl(
            STDOUT_FILENO,
            TIOCGWINSZ,
            (&mut winsize as *mut Winsize).cast(),
        )
    });
    ok &= winsize_ok;
    if winsize_ok {
        logl::log(
            level::INFO,
            format_args!(
                "unix-api-probe: winsize rows={} cols={} xpixel={} ypixel={}",
                winsize.ws_row, winsize.ws_col, winsize.ws_xpixel, winsize.ws_ypixel
            ),
        );
    }

    let mut available: c_int = 0;
    log_stage("ioctl.FIONREAD.stdin");
    clear_errno();
    ok &= log_rc_zero("ioctl.FIONREAD.stdin", unsafe {
        ioctl(
            STDIN_FILENO,
            FIONREAD,
            (&mut available as *mut c_int).cast(),
        )
    });

    let mut termios = [0u8; 256];
    log_stage("tcgetattr.stdin");
    clear_errno();
    let tcgetattr_ok = log_rc_zero("tcgetattr.stdin", unsafe {
        tcgetattr(STDIN_FILENO, termios.as_mut_ptr().cast())
    });
    ok &= tcgetattr_ok;
    if tcgetattr_ok {
        log_stage("tcsetattr.stdin.restore");
        clear_errno();
        ok &= log_rc_zero("tcsetattr.stdin.restore", unsafe {
            tcsetattr(STDIN_FILENO, TCSANOW, termios.as_ptr().cast())
        });
    }

    let mut raw_termios = [0u8; 256];
    log_stage("ioctl.TCGETS.stdin");
    clear_errno();
    ok &= log_rc_zero("ioctl.TCGETS.stdin", unsafe {
        ioctl(STDIN_FILENO, TCGETS, raw_termios.as_mut_ptr().cast())
    });

    ok
}

fn probe_pipe() -> bool {
    let mut ok = true;
    let mut fds = [-1, -1];

    log_stage("pipe");
    clear_errno();
    if !log_rc_zero("pipe", unsafe { pipe(fds.as_mut_ptr()) }) {
        return false;
    }
    if !pipe_fds_valid("pipe", fds) {
        return false;
    }
    ok &= log_fd("pipe.read_fd", fds[0]);
    ok &= log_fd("pipe.write_fd", fds[1]);

    log_stage("fcntl.pipe.read.F_GETFL");
    clear_errno();
    let read_flags = unsafe { fcntl(fds[0], F_GETFL, 0) };
    ok &= log_rc_nonnegative("fcntl.pipe.read.F_GETFL", read_flags);
    if read_flags >= 0 {
        log_stage("fcntl.pipe.read.F_SETFL_NONBLOCK");
        clear_errno();
        ok &= log_rc_zero("fcntl.pipe.read.F_SETFL_NONBLOCK", unsafe {
            fcntl(fds[0], F_SETFL, read_flags | O_NONBLOCK)
        });
    }

    let payload = b"pipe-ok";
    log_stage("write.pipe");
    clear_errno();
    ok &= log_io(
        "write.pipe",
        unsafe { write(fds[1], payload.as_ptr().cast(), payload.len()) },
        payload.len(),
    );

    let mut pollfds = [PollFd {
        fd: fds[0],
        events: POLLIN,
        revents: 0,
    }];
    log_stage("poll.pipe.readable");
    clear_errno();
    ok &= log_rc_nonnegative("poll.pipe.readable", unsafe {
        poll(pollfds.as_mut_ptr(), pollfds.len(), 0)
    });
    logl::log(
        level::INFO,
        format_args!("unix-api-probe: poll.pipe revents={}", pollfds[0].revents),
    );

    let mut buf = [0u8; 16];
    log_stage("read.pipe");
    clear_errno();
    ok &= log_io(
        "read.pipe",
        unsafe { read(fds[0], buf.as_mut_ptr().cast(), payload.len()) },
        payload.len(),
    );

    log_stage("close.pipe.read");
    clear_errno();
    ok &= log_rc_zero("close.pipe.read", unsafe { close(fds[0]) });
    log_stage("close.pipe.write");
    clear_errno();
    ok &= log_rc_zero("close.pipe.write", unsafe { close(fds[1]) });

    ok
}

fn probe_socketpair() -> bool {
    let mut ok = true;
    let mut fds = [-1, -1];

    log_stage("socketpair.AF_UNIX_STREAM");
    clear_errno();
    if !log_rc_zero("socketpair.AF_UNIX_STREAM", unsafe {
        socketpair(AF_UNIX, SOCK_STREAM, 0, fds.as_mut_ptr())
    }) {
        return false;
    }
    if !pipe_fds_valid("socketpair.AF_UNIX_STREAM", fds) {
        return false;
    }
    ok &= log_fd("socketpair.left", fds[0]);
    ok &= log_fd("socketpair.right", fds[1]);

    log_stage("fcntl.socket.left.F_GETFL");
    clear_errno();
    let left_flags = unsafe { fcntl(fds[0], F_GETFL, 0) };
    ok &= log_rc_nonnegative("fcntl.socket.left.F_GETFL", left_flags);
    if left_flags >= 0 {
        log_stage("fcntl.socket.left.F_SETFL_NONBLOCK");
        clear_errno();
        ok &= log_rc_zero("fcntl.socket.left.F_SETFL_NONBLOCK", unsafe {
            fcntl(fds[0], F_SETFL, left_flags | O_NONBLOCK)
        });
    }

    let payload = b"unix-stream-ok";
    log_stage("write.socketpair.right");
    clear_errno();
    ok &= log_io(
        "write.socketpair.right",
        unsafe { write(fds[1], payload.as_ptr().cast(), payload.len()) },
        payload.len(),
    );

    let mut pollfds = [
        PollFd {
            fd: fds[0],
            events: POLLIN,
            revents: 0,
        },
        PollFd {
            fd: fds[1],
            events: POLLOUT,
            revents: 0,
        },
    ];
    log_stage("poll.socketpair");
    clear_errno();
    ok &= log_rc_nonnegative("poll.socketpair", unsafe {
        poll(pollfds.as_mut_ptr(), pollfds.len(), 0)
    });
    logl::log(
        level::INFO,
        format_args!(
            "unix-api-probe: poll.socketpair left_revents={} right_revents={}",
            pollfds[0].revents, pollfds[1].revents
        ),
    );

    let mut buf = [0u8; 32];
    log_stage("read.socketpair.left");
    clear_errno();
    ok &= log_io(
        "read.socketpair.left",
        unsafe { read(fds[0], buf.as_mut_ptr().cast(), payload.len()) },
        payload.len(),
    );

    log_stage("close.socket.left");
    clear_errno();
    ok &= log_rc_zero("close.socket.left", unsafe { close(fds[0]) });
    log_stage("close.socket.right");
    clear_errno();
    ok &= log_rc_zero("close.socket.right", unsafe { close(fds[1]) });

    ok
}

fn run_probe() -> Result<(), &'static str> {
    let mut ok = true;
    ok &= probe_tty();
    ok &= probe_pipe();
    ok &= probe_socketpair();

    if ok {
        Ok(())
    } else {
        Err("one_or_more_unix_api_stages")
    }
}

fn main() {
    logl::log(level::INFO, "unix-api-probe: blueprint start");
    match run_probe() {
        Ok(()) => logl::log(level::INFO, "unix-api-probe: ok"),
        Err(stage) => logl::log(
            level::ERROR,
            format_args!("unix-api-probe: failed stage={}", stage),
        ),
    }
    platform::poll_once();
}
