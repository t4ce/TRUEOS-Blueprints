//! Compile the actual native SDK module with a recording CABI instead of a VM.
extern crate alloc;
extern crate self as v;

#[path = "../../src/command_session.rs"]
mod command_session;

#[derive(Default)]
struct Calls {
    opens: Vec<(String, u32, u32, u32)>,
    controls: Vec<(u64, u32, u32, u32)>,
    writes: Vec<Vec<u8>>,
}
static CALLS: std::sync::Mutex<Calls> = std::sync::Mutex::new(Calls {
    opens: Vec::new(),
    controls: Vec::new(),
    writes: Vec::new(),
});

pub mod vcabi {
    pub unsafe fn trueos_cabi_shell3_session_open_v1(
        script: *const u8,
        len: usize,
        cols: u32,
        rows: u32,
        input: u32,
    ) -> i64 {
        let script = unsafe { std::slice::from_raw_parts(script, len) };
        super::CALLS.lock().unwrap().opens.push((
            String::from_utf8(script.to_vec()).unwrap(),
            cols,
            rows,
            input,
        ));
        41
    }
    pub unsafe fn trueos_cabi_shell3_session_control_v1(
        session: u64,
        op: u32,
        cols: u32,
        rows: u32,
    ) -> i32 {
        super::CALLS
            .lock()
            .unwrap()
            .controls
            .push((session, op, cols, rows));
        0
    }
    pub unsafe fn trueos_cabi_shell3_session_read_v1(
        _session: u64,
        output: *mut u8,
        cap: usize,
    ) -> isize {
        let output = unsafe { std::slice::from_raw_parts_mut(output, cap) };
        let text = "ready §\n".as_bytes();
        assert!(output.len() >= 8 + text.len());
        output[..4].copy_from_slice(&1u32.to_le_bytes());
        output[4..8].copy_from_slice(&7i32.to_le_bytes());
        output[8..8 + text.len()].copy_from_slice(text);
        (8 + text.len()) as isize
    }
    pub unsafe fn trueos_cabi_shell3_session_write_v1(
        _session: u64,
        input: *const u8,
        len: usize,
    ) -> isize {
        let input = unsafe { std::slice::from_raw_parts(input, len) };
        super::CALLS.lock().unwrap().writes.push(input.to_vec());
        len as isize
    }
}

fn clear() {
    *CALLS.lock().unwrap() = Calls::default();
}

#[test]
fn native_absent_terminal_survives_as_zero_geometry_on_the_actual_wire() {
    use command_session::{KernelBackend, Launch, Session, State};
    clear();
    let script = "adm ram\nhv status\n";
    let mut session = Session::open(
        &KernelBackend,
        Launch {
            script: script.into(),
            terminal: None,
            stdin_open: false,
        },
    )
    .unwrap();
    assert_eq!(CALLS.lock().unwrap().opens, [(script.into(), 0, 0, 0)]);
    let mut output = [0u8; 64];
    let read = session.transport().read(&mut output).unwrap();
    assert_eq!(&output[..read.len], "ready §\n".as_bytes());
    assert_eq!(
        read.state,
        State::Exited(7),
        "final bytes and exit travel together"
    );
    drop(session);
    assert_eq!(CALLS.lock().unwrap().controls, [(41, 5, 0, 0)]);
}

#[test]
fn explicit_terminal_dimensions_and_controls_remain_unchanged() {
    use command_session::{Geometry, KernelBackend, Launch, Session, Signal};
    clear();
    let mut session = Session::open(
        &KernelBackend,
        Launch {
            script: "\tram".into(),
            terminal: Some(Geometry {
                cols: 120,
                rows: 40,
            }),
            stdin_open: true,
        },
    )
    .unwrap();
    assert_eq!(CALLS.lock().unwrap().opens, [("\tram".into(), 120, 40, 1)]);
    assert_eq!(session.transport().write(b"env\n").unwrap(), 4);
    session
        .transport()
        .resize(Geometry {
            cols: 140,
            rows: 50,
        })
        .unwrap();
    session.transport().close_stdin().unwrap();
    session.transport().signal(Signal::Interrupt).unwrap();
    session.transport().close();
    drop(session);
    let calls = CALLS.lock().unwrap();
    assert_eq!(calls.writes, [b"env\n".to_vec()]);
    assert_eq!(
        calls.controls,
        [
            (41, 1, 140, 50),
            (41, 2, 0, 0),
            (41, 3, 0, 0),
            (41, 5, 0, 0)
        ],
        "close is idempotent"
    );
}

#[test]
fn invalid_explicit_dimensions_do_not_silently_become_captured_sessions() {
    use command_session::{Error, Geometry, KernelBackend, Launch, Session};
    clear();
    for (cols, rows) in [(0, 40), (120, 0), (0, 0)] {
        assert!(matches!(
            Session::open(
                &KernelBackend,
                Launch {
                    script: "ram".into(),
                    terminal: Some(Geometry { cols, rows }),
                    stdin_open: false,
                }
            ),
            Err(Error::InvalidRequest)
        ));
    }
    assert!(CALLS.lock().unwrap().opens.is_empty());
}
