use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn mcp_session_over_real_stdio() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_robomcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    fn exchange(input: &mut impl Write, output: &mut impl BufRead, request: Value) -> Value {
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        assert!(output.read_line(&mut line).unwrap() > 0);
        serde_json::from_str(&line).unwrap()
    }
    let init = exchange(
        &mut input,
        &mut output,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}
        }),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["capabilities"], json!({"tools": {}}));
    writeln!(
        input,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .unwrap();
    let list = exchange(
        &mut input,
        &mut output,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    );
    assert_eq!(list["id"], 2); // initialized must not emit a reply
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(list["result"]["tools"][0]["name"], "ok");
    // Leave the pipe open with no input before another request.
    std::thread::sleep(std::time::Duration::from_millis(50));
    let ok = exchange(
        &mut input,
        &mut output,
        json!({"jsonrpc": "2.0", "id": "ok", "method": "tools/call", "params": {"name": "ok", "arguments": {}}}),
    );
    assert_eq!(ok["id"], "ok");
    assert_eq!(
        ok["result"]["content"],
        json!([{"type": "text", "text": "OK"}])
    );
    let unknown = exchange(
        &mut input,
        &mut output,
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "move_motor"}}),
    );
    assert_eq!(unknown["error"]["code"], -32602);
    let invalid = exchange(
        &mut input,
        &mut output,
        json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "ok", "arguments": {"motor": 1}}}),
    );
    assert_eq!(invalid["error"]["code"], -32602);
    writeln!(input, "invalid json").unwrap();
    let ping = exchange(
        &mut input,
        &mut output,
        json!({"jsonrpc": "2.0", "id": 6, "method": "ping"}),
    );
    assert_eq!(ping["error"]["code"], -32700);
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["id"], 6);
    drop(input);
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    assert!(result.stderr.is_empty());
}
