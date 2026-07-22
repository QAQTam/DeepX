# DeepX daemon architecture

DeepX is split into two user-facing applications:

- `deepx-daemon` owns sessions, configuration, workspaces, tools and Agent workers.
- `deepx-desktop` is the independent Electron/SolidJS client. Closing it does not stop the daemon or running work. The legacy Tauri shell was retired after Electron reached feature parity during the 0.9 transition.

The daemon binds an ephemeral loopback port and atomically publishes `%USERPROFILE%\.deepx\daemon.json` (or the XDG data directory on Unix). The document contains the endpoint, process ID, protocol version, server epoch and a per-launch bearer token. Only the daemon and native clients read or write DeepX business data.

## Control protocol

`deepx-proto::control` defines the versioned WebSocket protocol. A client authenticates during the WebSocket handshake, sends `client_hello`, then receives `server_hello` followed by either replayed ordered events or a snapshot. Requests and responses share a `request_id`; streaming Agent events carry global and per-session sequence numbers.

Sessions use exclusive leases. One client instance can attach to a session, heartbeats renew the lease every five seconds, and a disconnected client's lease expires after fifteen seconds. Other clients can list sessions and activity but receive `session_busy` when attaching to an occupied session.

## Development

```powershell
just dev
deepx-daemon status
deepx-daemon stop
```

`deepx-client` contains discovery, authentication, request correlation, heartbeat and reconnect cursor support. A future TUI should depend on this crate rather than accessing `.deepx` files directly. `cargo run -p deepx-client --example daemon_probe -- <path-to-deepx-daemon>` is a minimal headless connectivity probe.

The Companion/Pet endpoint is intentionally not started in this version. Its crate remains in the workspace for a future daemon-hosted reintroduction.
