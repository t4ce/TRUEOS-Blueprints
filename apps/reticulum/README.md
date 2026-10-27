
# Reticulum-rs

**Reticulum-rs** is a Rust implementation of the [Reticulum Network Stack](https://reticulum.network/) — a cryptographic, decentralised, and resilient mesh networking protocol designed for communication over any physical layer.

This project brings Reticulum's capabilities to the Rust ecosystem, enabling embedded, and constrained deployments with maximum performance and minimal dependencies.

## Features

- 📡 Cryptographic mesh networking
- 🔐 Trustless routing via identity-based keys
- 📁 Lightweight and modular design
- 🧱 Support for multiple transport layers (TCP, serial, Kaonic)
- 🔌 Easily embeddable in embedded devices and tactical radios
- 🧪 Example clients for testnets and real deployments

## Structure


```
Reticulum-rs/
├── src/                 # Core Reticulum protocol implementation
│   ├── buffer.rs
│   ├── crypt.rs
│   ├── destination.rs
│   ├── error.rs
│   ├── hash.rs
│   ├── identity.rs
│   ├── iface.rs
│   ├── lib.rs
│   ├── transport.rs
│   └── packet.rs
├── proto/               # Protocol definitions (e.g. for Kaonic)
│   └── kaonic/
│       └── kaonic.proto
├── examples/            # Example clients and servers
│   ├── kaonic_client.rs
│   ├── link_client.rs
│   ├── tcp_client.rs
│   ├── tcp_server.rs
│   └── testnet_client.rs
├── reticulum-daemon/           # RNS Daemon
│   ├── Cargo.toml
│   └── src/
│       ├── config.rs
│       └── main.rs
├── Cargo.toml           # Crate configuration
├── LICENSE              # License (MIT/Apache)
└── build.rs             
````
## Getting Started

### Prerequisites

* Rust (edition 2021+)
* `protoc` for compiling `.proto` files (if using gRPC/Kaonic modules)

### Build

```bash
cargo build --release
```

### TRUEOS Blueprint

From the TRUEOS-Blueprints root, run `cargo bp reticulum` to pack
`dist/reticulum.bp`. The `trueos-blueprint` feature selects the existing
TRUEOS dual TCP probe in `src/blueprint.rs`, including its logging and runtime
setup. The library in `src/lib.rs` remains available to other crates. This probe
is separate from the host `reticulum-daemon` CLI.

### Reticulum daemon

#### Converting config from Python Reticulum

Reticulum-rs uses TOML for configuration, whereas the original Python Reticulum uses a custom format parsed by configobj, a Python-only library. If you have an existing Python Reticulum configuration, it will be read and converted to TOML in-memory. If you want to apply the conversion and save a TOML copy, run the `convert-config` subcommand:

```bash
cargo run -p reticulum-daemon -- convert-config <config_file>
```

This leaves the original file and creates a copy with .toml extension. The converter handles boolean normalization (True/False/Yes/No → true/false), quotes string values, transforms interface declarations to TOML array-of-tables syntax, and comments out None/nil values which TOML does not support.

#### Running the daemon

```bash
# Use default config search paths (~/.config/reticulum, ~/.reticulum, /etc/reticulum)
cargo run -p reticulum-daemon

# Specify a custom config directory
cargo run -p reticulum-daemon -- --config /path/to/config/dir
cargo run -p reticulum-daemon -- -c /path/to/config/dir
```

The daemon searches for either `config` (legacy filename) or `config.toml` in the specified directory.

### Run Examples

```bash
# TCP client example
cargo run --example tcp_client

# Kaonic mesh test client
cargo run --example kaonic_client
```

### Python integration tests

Integration tests against the Python implementation can be run with the `python-tests` feature and
setting the `RETICULUM_TEST_PYTHON_DIR` environment variable to the location of the checked out
Python Reticulum source tree. Example:
```
RETICULUM_TEST_PYTHON_DIR=../Reticulum cargo test python --features="python-tests"
```

## Use Cases

* 🛰 Tactical radio mesh with Kaonic
* 🕵️‍♂️ Covert communication using serial or sub-GHz transceivers
* 🚁 UAV-to-ground resilient C2 and telemetry
* 🧱 Decentralized infrastructure-free messaging

## License

This project is licensed under the MIT license.

---

© Beechat Network Systems Ltd. All rights reserved.
https://beechat.network/
