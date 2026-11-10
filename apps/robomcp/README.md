# RoboMCP

On-demand TRUEOS Blueprint providing the starting boundary for a future robotics
MCP server. Its only tool is `ok`, with no arguments, returning `OK`. There is no
hardware access, motor control, device discovery, storage, or robotics abstraction
yet. Future robotics tools belong behind this boundary; they are not implemented
by this skeleton.

The process speaks newline-delimited JSON-RPC over stdio, with MCP initialization,
ping, tool listing and tool calls. Stdout contains protocol messages only. Messages
are limited to 64 KiB. Host stdin EOF ends the process; on TRUEOS, the native stdio
backend follows the parent/child lifecycle. Each launch is an independent instance
with no persistent state.

## Build and verify

From the TRUEOS-Blueprints repository:

```sh
cargo test -p robomcp
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp robomcp
```

The Blueprint build produces `dist/robomcp.bp`. Publish it through the usual
Blueprint app catalog workflow when ready for use on a running TRUEOS system.

## TRUEOS Codex integration

TRUEOS Codex's packaged defaults register `robomcp` as disabled and optional.
After the Blueprint is available in the app catalog, opt in for a session:

```sh
codex -c mcp_servers.robomcp.enabled=true
```

Or enable it in the TRUEOS Codex user configuration:

```toml
[mcp_servers.robomcp]
enabled = true
```

Codex launches `robomcp` through its existing native child stdio transport. The
Blueprint is cataloged separately and is not a mandatory Codex bundle dependency.
The disabled default starts no process. This is a Blueprint, not a buildin.

For a host Codex smoke test, first run `cargo build -p robomcp`, then register
the absolute path to `target/x86_64-unknown-linux-gnu/debug/robomcp` as a stdio
MCP server using `codex mcp add robomcp -- /absolute/path/to/robomcp`.

Protocol references: [stdio transport](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports)
and [initialization lifecycle](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle).
