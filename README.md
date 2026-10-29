# `pyre-python-engine`

> **Core Environment Auto-Detection & Process Execution Subsystem for APW Python IDE**

[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Status](https://img.shields.io/badge/status-active%20development-green.svg)]()

---

## Overview

`pyre-python-engine` is the high-performance, asynchronous Rust subsystem powering the **APW Python IDE**. It serves as the bridge between the IDE UI layer and target Python execution environments, handling zero-configuration interpreter discovery and sandboxed script execution.

It is designed for strict error isolation, low-latency asynchronous I/O, cross-platform compatibility, and zero process leaks.

---

## Key Features

-  **Zero-Config Interpreter Auto-Detection**:
  - Automatically scans workspace roots for `.venv`, `venv`, and `env` virtual environments.
  - Introspects system paths (`PATH`), Conda environments (`conda env list`), and `pyenv` installations.
  - Runs isolated metadata checks (`python -I`) to safely query executable version info, prefix paths, and platform metadata in JSON format.
-  **Async Process Supervision**:
  - Spawns Python scripts and modules securely inside target environments.
  - Asynchronously streams `stdout` and `stderr` line-by-line using Tokio non-blocking channels to eliminate pipe deadlocks and UI freezes.
-  **Cross-Platform Orphan Cleanup**:
  - Proper process group signal management (`nix::sys::signal` / `setpgid`) on Unix/Linux/macOS.
  - Process tree termination (`taskkill /F /T`) on Windows to prevent runaway background processes.
-  **Production-Grade Architecture**:
  - Robust error propagation via `thiserror` (zero `unwrap()` or `panic!` calls in production paths).
  - Structured logging via `tracing`.

---

##  Architecture & Module Structure

```text
pyre-python-engine/
├── Cargo.toml
└── src/
    ├── lib.rs          # Public module exports and integration test fixtures
    ├── error.rs        # Strongly typed EngineError enum (thiserror)
    ├── interpreter.rs  # Virtualenv scanner & isolated interpreter validation
    └── supervisor.rs   # Process supervisor, stream redirection, and tree cleanup
```

---

##  Technology Stack

- **Language**: Rust (2021 Edition)
- **Async Runtime**: [Tokio](https://tokio.rs/) (Process management, async I/O, timers)
- **Serialization**: `serde`, `serde_json`
- **Path Lookup**: `which`, `dirs`
- **Error Handling & Logging**: `thiserror`, `tracing`
- **POSIX Signal Handling**: `nix` (Unix targets)

---

## Quickstart & Development

### Prerequisites

- [Rust Toolchain](https://rustup.rs/) (1.75+ recommended)
- Python 3.8+ installed locally

### Build

```bash
# Build debug binary
cargo build

# Build optimized release artifact
cargo build --release
```

### Run Tests

```bash
# Run unit and integration tests
cargo test

# Run tests with output logging
cargo test -- --nocapture
```

---

## Usage Example

```rust
use apw_python_engine::{InterpreterScanner, ProcessSupervisor, ProcessOutput};
use std::path::Path;
use std::collections::HashMap;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_path = Path::new("./my-python-project");

    // 1. Auto-discover available Python interpreters
    let interpreters = InterpreterScanner::discover_all(workspace_path).await;
    let target_python = &interpreters[0].executable;

    // 2. Spawn a script under supervision
    let script = workspace_path.join("main.py");
    let (mut supervisor, mut rx) = ProcessSupervisor::spawn(
        target_python,
        &script,
        &[],
        &HashMap::new(),
        Some(workspace_path),
    ).await?;

    // 3. Process live stdout/stderr stream
    tokio::spawn(async move {
        while let Some(output) = rx.recv().await {
            match output {
                ProcessOutput::Stdout(line) => println!("[STDOUT] {}", line),
                ProcessOutput::Stderr(line) => eprintln!("[STDERR] {}", line),
            }
        }
    });

    // 4. Wait for exit with timeout
    let exit_code = supervisor.wait_with_timeout(Duration::from_secs(30)).await?;
    println!("Process finished with exit code: {}", exit_code);

    Ok(())
}
```

---

## Team Contacts & Module Ownership

- **Lead Architecture & Engine**: APW Labs Engineering (`engineering@apwlabs.io`)
- **Core Components**: `InterpreterScanner`, `ProcessSupervisor`

---

## License

Distributed under the MIT License. See `LICENSE` for details.