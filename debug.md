# deepx agent 0xc0000005 Access Violation — Debug Report

## Problem Summary

deepx agent child process (`deepx.exe`) on Windows x64 crashes with `0xc0000005` Access Violation after 3-4 rounds of tool-intensive conversation, at the point where the LLM returns its final answer (Done event). Both Debug and Release builds are affected. Both TUI and Tauri frontends reproduce the issue.

## Windows Event Viewer Crash Records

4 crash events observed from `Application Error` log:

### Crash #1 (Release build)
```
AppName: deepx.exe (0.5.0.0)
ModuleName: deepx.exe
ExceptionCode: 0xc0000005
FaultingOffset: 0x0000000000749eb8
AppPath: D:\project\DeepX\target\release\deepx.exe
```

### Crash #2 (Debug build)
```
AppName: deepx.exe (0.5.0.0)
ModuleName: deepx.exe
ExceptionCode: 0xc0000005
FaultingOffset: 0x00000000013a5758
AppPath: D:\project\DeepX\target\debug\deepx.exe
```

### Crash #3 (Debug build, after heap-allocation fix)
```
AppName: deepx.exe (0.5.0.0)
ModuleName: deepx.exe
ExceptionCode: 0xc0000005
FaultingOffset: 0x00000000013a5658
AppPath: D:\project\DeepX\target\debug\deepx.exe
```

### Crash #4 (Debug build, after 8MB stack commit fix)
```
AppName: deepx.exe (0.5.0.0)
ModuleName: deepx.exe
ExceptionCode: 0xc0000005
FaultingOffset: 0x00000000013a5658
AppPath: D:\project\DeepX\target\debug\deepx.exe
```

## PE Header (Debug build, default)

```
size of stack reserve: 0x100000 (1MB)
size of stack commit:  0x1000   (4KB)
```

Overridden via `.cargo/config.toml`:

```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "link-args=/STACK:0x800000,0x800000"]
```

Result: reserve=8MB, commit=8MB. Crash #4 occurred after this change.

## dumpbin Disassembly (Debug build)

Crash offset `0x13a5658` maps to `.text` section VA `0x1413A6258`:

```
core::result::Result<T,E> as core::ops::try_trait::Try::branch (hash 6cc2edad57903488):
  00000001413A62C0: 48 83 EC 30        sub  rsp, 0x30
  00000001413A62C4: 48 89 14 24        mov  qword ptr [rsp], rdx
  00000001413A62C8: 48 89 4C 24 08     mov  qword ptr [rsp+8], rcx   <-- crash here (#3, #4)
```

This function is Rust stdlib `core::result::Result::Try::branch` (the `?` operator implementation). Crash occurs on first stack write after `sub rsp, 0x30`.

Additionally, a different monomorphized instance of the same function crashed at a nearby address:

```
core::result::Result<T,E> as core::ops::try_trait::Try::branch (hash 2c6bdc61c5045c12):
  00000001413A6350: 48 83 EC 30        sub  rsp, 0x30
  00000001413A6358: 48 89 4C 24 08     mov  qword ptr [rsp+8], rcx   <-- crash here (#2)
```

## Binary Analysis

- `__chkstk` symbol not present in binary (MSVC stack probes not enabled)
- `.text` section contains 811 functions with stack frame >= 2KB (Debug) / 180 functions (Release), max ~4080 bytes
- `deepx_tools::file_read::exec_read_file` has stack frame `sub rsp, 0xFF0` (4080 bytes)
- `[u8; 4096]` in `openai.rs:202` and `exec.rs:53` were moved from stack to heap (`vec![0u8; 4096]`); crashes still occur

## Modifications Applied (current HEAD: v0.5.0)

1. `exec.rs:53`, `openai.rs:202`: `[u8; 4096]` -> `vec![0u8; 4096]` (heap allocation)
2. `deepx-msglp/src/lib.rs`: `catch_unwind` added to reader/writer threads
3. `deepx-msglp/src/lib.rs`: `writer_dead` AtomicBool detection in main loop
4. `deepx-msglp/src/lib.rs`: `emit()` early-return checks `writer_dead`
5. `deepx-msglp/src/lib.rs`: sync_channel capacity 256->4096
6. `deepx-msglp/src/lib.rs`: `CodeDelta`/`AuditRecord`/`Dashboard`/`ToolNotice` changed to `emit_delta` (try_send)
7. `deepx-msglp/src/lib.rs`: main loop dispatch wrapped in `catch_unwind`
8. `deepx-tauri/src-tauri/src/main.rs`: `run()` wrapped in `catch_unwind`
9. `.cargo/config.toml`: linker args `/STACK:0x800000,0x800000`

## Crash Characteristics

- Exception: `0xc0000005` (STATUS_ACCESS_VIOLATION)
- Crash location: `core::result::Result::Try::branch` (Rust `?` operator)
- Crash instruction: `mov qword ptr [rsp+8], rcx` (stack pointer-relative write)
- Stack space: reserve=8MB, commit=8MB (rules out stack exhaustion)
- Trigger: 3-4 tool rounds, at LLM final answer (Done event)
- Platform: Windows x64 MSVC, Rust 1.96.0 stable
- Scope: Debug + Release builds, TUI + Tauri frontends
- Direct ToolCall injection (bypassing LLM API call chain) does not trigger crash

## Information Not Yet Obtained

- WinDbg register values at crash (`rsp`, `rcx`, `rdx`)
- WinDbg call stack at crash (`k` command output)
- Whether crashing thread is the agent main thread
- Whether `rsp` value at crash is within valid stack range (8MB commit)

## Git History (prior to first crash at 2026-06-30 23:24:44 +0800)

Commits on June 30, 2026 in chronological order:

```
ac9892e 2026-06-30 10:32:05 clean
762a564 2026-06-30 10:37:12 clean code
ba45f34 2026-06-30 10:38:02 clean
defe4b6 2026-06-30 11:07:14 fix
f9d8a2b 2026-06-30 16:19:20 feat: comprehensive tool & architecture overhaul
2646429 2026-06-30 16:26:58 fix: use non-blocking wait for child process exit detection
b835450 2026-06-30 17:54:17 fix
4b0176a 2026-06-30 23:13:49 feat: PTY stdin write, cross-session memory, prompt reformat
2a25740 2026-06-30 23:14:17 chore: bump version 0.4.0 -> 0.5.0
```

First crash timestamp: 2026-06-30 23:24:44 (from eventvwr)

### f9d8a2b commit message

```
feat: comprehensive tool & architecture overhaul

## File tools (deepx-tools)
- Fix concurrent same-file edits: conflict detection + serialization in msg loop
- Add resolve_workspace_path() to all file tools for ./ relative path support
- Unify output prefixes: [OK] on explore, list_dir, search, diff, process_inspect
- Delete MCP bridge (mcp_bridge.rs + config fields)

## Workspace (deepx-msglp)
- Fix ReloadConfig dropped during busy: add pending_reload_config queue
- Inject workspace path as [Environment] annotation per user message

## Exec tool
- Fix character swallowing: replace read_line() with read() + manual line splitting
- Handle \r progress-bar overwrite semantics at source
- Throttle ExecProgress emit to 50ms batches
- Use pwsh -EncodedCommand (Base64 UTF-16LE) to bypass string parsing

## Daemon architecture (deepx-daemon, new)
- deepxd: background service managing agent process pool
- Agent lifecycle: spawn, health check, auto-restart (max 3), idle reap (30min)
- Tauri: daemon reader thread + send_to_agent/ensure_agent daemon-first fallback
- TUI: spawn_agent_subprocess daemon-first connection
```

### f9d8a2b: Files changed in deepx-msglp (the agent message loop)

```
crates/deepx-msglp/src/agent.rs      |  15 +-
crates/deepx-msglp/src/lib.rs        | 158 +-
crates/deepx-msglp/src/lifecycle.rs  |  12 +-
```

`lib.rs` changes (158 lines net added) include:
- Added `file_write_paths()` function for same-file write conflict detection
- Added `pending_reload_config` field to `Loop` struct
- Added parallel tool execution with conflict detection, thread spawning, drain loop with 50ms batching
- Added serialized follow-up tool execution for same-file write conflicts
- Added workspace annotation injection into user messages

### f9d8a2b: Key dependency changes

```
Cargo.lock: 17 lines added (new deps: deepx-daemon, base64, windows-sys)
```

No existing dependency versions changed. Only new crates added.

## Crash Call Path (crate dependency chain)

The crash occurs at `core::result::Result::Try::branch` (Rust `?` operator). The only `?` operators on the Done emission path are in `deepx-session/src/store.rs:append_messages`:

```
deepx-tauri/src-tauri/src/main.rs:run_agent()
  → deepx-msglp/src/lib.rs:Loop::new_ipc()
    → deepx-msglp/src/lib.rs:Loop::run()
      → deepx-msglp/src/lib.rs:dispatch(Ui2Agent::UserInput)
        → deepx-msglp/src/lib.rs:handle_user_input()
          → [round loop]
            → deepx-msglp/src/agent.rs:build_context()
              → deepx-message/src/store.rs:build_context_for_gate()
            → deepx-gate/src/openai.rs:chat_stream()
              → deepx-gate/src/openai.rs:chat_stream_openai()
                → deepx-gate/src/openai.rs:stream_sse()  [contains byte_buf: Vec<u8> 4KB heap buffer]
                  → closure callback in handle_user_input (lib.rs:~862)
            → deepx-gate/src/tool_parser.rs:parse_tool_calls_from_response()
            → deepx-message/src/store.rs:push_assistant()
            → [tool execution: thread::spawn + drain loop + join]
              → deepx-tools/src/bridge.rs:execute_tool_with_id_full()
                → deepx-tools/src/file_read.rs:exec_read_file()  [4080-byte stack frame]
            → deepx-message/src/store.rs:push_tool_result_direct()
          → deepx-msglp/src/lib.rs:flush_meta_and_stats()
            → deepx-message/src/store.rs:flush_meta()
              → deepx-session/src/manager.rs:save_append()
                → deepx-session/src/store.rs:append_messages()
                  → serde_json::to_string(msg)?   ← `?` operator, calls Try::branch
                  → writeln!(file, ...)?
                  → file.flush()?
                  → file.sync_all()?
          → emit(TurnEnd)  [blocking SyncSender::send]
          → emit(Done)      [blocking SyncSender::send, at lib.rs ~1239]
```

Key observations:
- `deepx-msglp/src/lib.rs` and `deepx-message/src/store.rs` contain zero `?` operators
- `deepx-session/src/store.rs::append_messages` contains 4 sequential `?` operators for file I/O
- `deepx-tools/src/file_read.rs::exec_read_file` has a 4080-byte stack frame (`sub rsp, 0xFF0`)

- `deepx-gate/src/openai.rs::stream_sse` has a 4KB heap buffer (`Vec<u8>`, was `[u8;4096]` before fix)

## Analysis & Fix (2026-07-01)

### Root Cause Analysis

The crash signature (`0xc0000005` at `Try::branch` writing `[rsp+8]`) was
investigated across multiple hypotheses:

1. **Stack exhaustion** — Ruled out: 8MB commit on main thread; no function
   has `sub rsp` ≥ 4096 (max 4080), so no single frame can skip a guard page.
2. **Missing `__chkstk`** — Confirmed absent from binary, but Rust 1.96.0 /
   LLVM 22.1.2 likely uses inline probes. Individual frames < 4096 bytes
   cannot skip a 4KB guard page regardless.
3. **conpty 0.7.0 UB** — `Process::Drop` does `Box::from_raw(ptr as *mut u8)`
   to free a `vec![0u8; size]` allocation (alloc `size` / dealloc 1). This is
   UB, but Windows `HeapFree` ignores the size parameter, so it does not
   corrupt the heap under the default `System` allocator.
4. **`pending_save` accumulation** — **Most likely contributor.** In the round
   loop, `flush_meta_and_stats` was only called on `Effect::TurnComplete`
   (final round). During 3-4 tool rounds, `pending_save` accumulated all
   messages including large tool results (up to 1MB each). The subsequent
   `append_messages` → `serde_json::to_string(msg)` for each message created
   heavy heap pressure. Direct ToolCall injection (`handle_tool_call`) does
   NOT call `flush_meta_and_stats`, which explains why it never crashes.
5. **Tool thread stack** — Default 2MB; `exec_read_file` alone has a 4080-byte
   frame. Deep call chains under pressure could approach limits.
6. **`strip_ansi` UTF-8 corruption** — `bytes[i] as char` in `exec.rs` breaks
   multi-byte UTF-8 (CJK, emoji), producing garbled `String`s that consume
   2× memory and may stress the allocator.

### Fixes Applied

**`crates/deepx-msglp/src/lib.rs`:**
1. **Per-round flush** — Added `self.flush_meta_and_stats()` in the
   `Effect::None` continue path of `handle_user_input`'s round loop. This
   prevents `pending_save` from accumulating across rounds, dramatically
   reducing heap pressure during `append_messages`.
2. **4MB tool thread stack** — Replaced `std::thread::spawn` with
   `thread::Builder::new().stack_size(4 * 1024 * 1024).spawn(...)` at both
   tool execution sites (`handle_tool_call` ~line 627, `handle_user_input`
   parallel tools ~line 1059). Ensures adequate stack for deep tool call
   chains including `exec_read_file`'s 4080-byte frame.

**`crates/deepx-tools/src/exec.rs`:**
3. **UTF-8 safe `strip_ansi`** — Replaced `out.push(bytes[i] as char)` with
   `out.push_str(&s[start..i])` to preserve multi-byte UTF-8 sequences in
   PTY output. The old code treated each byte as a separate Unicode codepoint,
   corrupting CJK text and inflating string sizes.

### Known Issues (not fixed)

- **conpty 0.7.0 `Box::from_raw` UB** — `Process::Drop` frees a multi-byte
  allocation as `Box<u8>` (1 byte). Not directly harmful under Windows
  `HeapFree`, but should be fixed by upgrading conpty or switching to
  `portable-pty`.
- **`PipeReader::Drop` uses `.unwrap()`** — `CloseHandle` failure panics in
  the reader thread. Not in the main thread, so does not cause the observed
  crash, but should be reported upstream.
- **Exact crash address mapping** — The `FaultingOffset` values do not
  precisely match `Try::branch` VAs (off by 0xC00-0xC70), suggesting the
  crash may actually be in a nearby function. WinDbg `k` command output is
  needed for definitive mapping.
