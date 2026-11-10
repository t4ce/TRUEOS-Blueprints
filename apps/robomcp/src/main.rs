//! On-demand robotics MCP boundary. Only a connectivity check is implemented.
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Read, Write};

const MAX_MESSAGE_BYTES: u64 = 64 * 1024;

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn dispatch(request: Value) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str);
    if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || method.is_none()
        || id
            .as_ref()
            .is_some_and(|id| !id.is_string() && !id.is_number() && !id.is_null())
    {
        return Some(error(Value::Null, -32600, "Invalid Request"));
    }
    // Notifications, including initialized/cancelled, never get a response.
    let id = id?;
    let params = request.get("params");
    if params.is_some_and(|params| !params.is_object()) {
        return Some(error(id, -32602, "Invalid params"));
    }
    let result = match method.unwrap() {
        "initialize" => {
            let Some(version) = request
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
            else {
                return Some(error(id, -32602, "protocolVersion is required"));
            };
            let version = match version {
                "2024-11-05" | "2025-03-26" | "2025-06-18" => version,
                _ => "2025-06-18",
            };
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "robomcp", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "RoboMCP skeleton: only ok is available. Robotics hardware and motor control are not implemented."
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": [{
            "name": "ok",
            "description": "Check that the RoboMCP skeleton is connected. Returns OK; has no side effects.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        }]}),
        "tools/call" => {
            if request.pointer("/params/name").and_then(Value::as_str) != Some("ok") {
                return Some(error(id, -32602, "Unknown tool"));
            }
            if request
                .pointer("/params/arguments")
                .is_some_and(|args| !args.as_object().is_some_and(|object| object.is_empty()))
            {
                return Some(error(id, -32602, "ok takes no arguments"));
            }
            json!({"content": [{"type": "text", "text": "OK"}], "isError": false})
        }
        _ => return Some(error(id, -32601, "Method not found")),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn serve(input: impl Read, mut output: impl Write) -> io::Result<()> {
    let mut input = BufReader::new(input);
    loop {
        let mut line = Vec::new();
        let count = input
            .by_ref()
            .take(MAX_MESSAGE_BYTES + 1)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(());
        }
        if count as u64 > MAX_MESSAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP message exceeds 64 KiB",
            ));
        }
        let response = match serde_json::from_slice(&line) {
            Ok(request) => dispatch(request),
            Err(_) => Some(error(Value::Null, -32700, "Parse error")),
        };
        if let Some(response) = response {
            let bytes = serde_json::to_vec(&response)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            output.write_all(&bytes)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

fn main() -> io::Result<()> {
    serve(io::stdin().lock(), io::stdout().lock())
}
