//! AgentState: core agent session state backing the message loop.

use deepx_config::Config;
use deepx_session::SessionMeta;

use deepx_message::{ToolExecRequest, ToolExecReport};
use deepx_tools::bridge;

#[derive(Debug)]
pub struct AgentState {
    pub msg: deepx_message::MessageStore,
    pub config: deepx_config::Config,
    pub session: SessionMeta,
    pub tool_defs: Vec<deepx_types::ToolDef>,
    pub dsml_compat_count: u32,
    pub turn_count: u32,
    /// If true, skip all disk persistence (subagent disposable mode).
    pub ephemeral: bool,
}

impl AgentState {
    pub fn new(config: deepx_config::Config) -> Self {
        // Seed is empty until create_session / init_session assigns a real one.
        // This prevents accidental persistence of a placeholder seed.
        let msg = deepx_message::MessageStore::new("");
        Self {
            msg, config,
            session: SessionMeta::default(),
            tool_defs: Vec::new(),
            dsml_compat_count: 0,
            turn_count: 0,
            ephemeral: false,
        }
    }

    pub fn init(caller: &str) -> Self {
        let config = match Config::load() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("deepx-agent: Config::load failed ({e}), using default config");
                Config::default()
            }
        };
        bridge::init_tools(caller, &[deepx_subagent::register], vec![]);
        if let Some(ref key) = config.context7_api_key {
            if !key.is_empty() { bridge::set_context7_key(key); }
        }
        let mut agent = Self::new(config);
        agent.tool_defs = bridge::all_tools(); // full set — LLM sees all, execution filtered by ToolManager
        agent
    }

    /// Initialize agent in subagent mode with a restricted tool allowlist and optional ephemeral flag.
    /// The LLM sees ALL tools (cache-friendly); the ToolManager enforces the allowlist at execution.
    pub fn init_subagent(allowed_tools: &[String], ephemeral: bool) -> Self {
        let config = match Config::load() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("deepx-agent: Config::load failed ({e}), using default config");
                Config::default()
            }
        };
        bridge::init_tools("subagent", &[deepx_subagent::register], allowed_tools.to_vec());
        if let Some(ref key) = config.context7_api_key {
            if !key.is_empty() { bridge::set_context7_key(key); }
        }
        let mut agent = Self::new(config);
        agent.ephemeral = ephemeral;
        agent.tool_defs = bridge::all_tools(); // full set — LLM cache friendly
        agent
    }

    pub fn build_context(&mut self) -> Vec<deepx_types::Message> {
        let mut annotations: Vec<String> = Vec::new();
        let ws = deepx_tools::CURRENT_WORKSPACE.read().unwrap();
        if !ws.is_empty() && *ws != "." {
            annotations.push(format!(
                "<workspace_path>{ws}</workspace_path>"
            ));
        }
        // Future extensibility:
        // annotations.push("<active_agent>Alpha</active_agent>");
        // annotations.push("<teammates><agent>Beta</agent></teammates>");
        self.msg.build_context_for_gate("", &annotations)
    }

    pub fn rebind_store(&mut self) {
        self.msg.set_tool_executor(Box::new(|req: ToolExecRequest| {
            let result = deepx_tools::bridge::execute_tool_with_id(&req.name, "", &req.args.to_string(), &req.id);
            let success = !result.starts_with("[ERROR]") && !result.starts_with("[FAIL]");
            ToolExecReport { content: result, success, files_affected: Vec::new() }
        }));
    }

    pub fn maybe_save_session(&mut self) {
        if self.msg.has_pending_tools() { return; }
        self.msg.flush_meta(&self.config.model, &self.config.reasoning_effort);
    }
}
