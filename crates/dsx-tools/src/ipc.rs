//! JSON-LP IPC protocol transport layer.
//!
//! Frame types (`AgentToTools`, `ToolsToAgent`) and I/O helpers
//! (`read_frame`, `write_frame`) come from `dsx-proto`.
//!
//! Agent → Tools: tools_init, tool_call_req, tool_cancel, tools_shutdown
//! Tools → Agent: tools_ready, tool_progress, tool_result, tool_result_message, tool_error

use std::io;
use dsx_proto::{self, AgentToTools, ToolsToAgent};

use crate::ToolManager;

/// IPC main loop: read Agent frames from stdin → route via ToolManager → write to stdout.
///
/// Returns only on Shutdown or EOF.
pub fn ipc_main_loop(manager: &mut ToolManager) {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut writer = stdout.lock();

    loop {
        let frame = match dsx_proto::read_frame::<AgentToTools>(&mut reader) {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                let _ = dsx_proto::write_frame(&mut writer, &ToolsToAgent::ToolError {
                    id: "ipc".into(),
                    error: format!("IPC parse error: {}", e),
                    code: "IPC_ERROR".into(),
                });
                continue;
            }
        };

        match frame {
            AgentToTools::Init { allowed_tools, session_seed, auto_mode } => {
                manager.apply_init(allowed_tools, &session_seed, auto_mode);
                let tools = manager.filtered_defs();
                if dsx_proto::write_frame(&mut writer, &ToolsToAgent::Ready { tools }).is_err() {
                    break;
                }
            }

            AgentToTools::CallReq { id, name, action, args, timeout_secs } => {
                let response = manager.handle_req(id, &name, &action, args, timeout_secs);
                if dsx_proto::write_frame(&mut writer, &response).is_err() {
                    break;
                }
            }

            AgentToTools::Cancel { id } => {
                manager.cancel_tool(id.as_deref());
            }

            AgentToTools::Shutdown => break,

            _ => continue, // #[non_exhaustive] guard
        }
    }
}
