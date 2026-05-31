# DeepX

A Windows-first terminal AI agent with 1M-token context, powered by HP Agents platform.

## Architecture

This workspace contains 7 crates:

| Crate | Description |
|---|---|
| `dsx` | Entry binary |
| `dsx-agent` | Core agent logic: session, orchestration, tool parser |
| `dsx-hp` | Health Platform runtime: process registry, liveness, OpenAI API |
| `dsx-proto` | Protocol definitions (Agent↔Tools, Agent↔HP) |
| `dsx-tools` | Built-in tools: exec, explore, file, web, plan, task, diff |
| `dsx-tui` | Terminal UI interface |
| `dsx-types` | Shared types |

## Getting Started

```bash
cargo build --release
./target/release/dsx
```
