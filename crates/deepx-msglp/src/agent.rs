//! AgentState: core agent session state (post-modularization shell).

use deepx_config::Config;

use dsx_message::MessageStore;


#[derive(Debug)]
pub struct AgentState {
    pub msg: dsx_message::MessageStore,
    pub config: deepx_config::Config,
    pub session: dsx_session::SessionMeta,
    pub tool_results: Vec<(String, String)>,
    pub tool_defs: Vec<dsx_types::ToolDef>,
    pub dsml_compat_count: u32,
    pub turn_count: u32,
}

impl AgentState {
    pub fn new(config: deepx_config::Config) -> Self {
        let mut msg = dsx_message::MessageStore::new("init");
        msg.push_system(dsx_types::Message::system(&deepx_config::prompt::system_prompt()));
        Self {
            msg, config,
            session: SessionMeta::new(),
            tool_results: Vec::new(),
            tool_defs: Vec::new(),
            dsml_compat_count: 0,
            turn_count: 0,
        }
    }

    pub fn init(caller: &str) -> Self {
        let config = Config::load().unwrap_or_default();
        dsx_tools::init_tools(caller, &[]);
        if let Some(ref key) = config.context7_api_key {
            if !key.is_empty() { dsx_tools::set_context7_key(key); }
        }
        let mut agent = Self::new(config);
        agent.tool_defs = dsx_tools::all_tools();
        agent
    }

    pub fn build_context(&mut self) -> Vec<dsx_types::Message> {
        let mut sys = String::new();
        if !self.session.from_resume {
            if self.config.reasoning_effort == "max" {
                sys.push_str(deepx_config::prompt::THINK_MAX);
                sys.push('\n');
            }
            sys.push_str(&deepx_config::prompt::system_prompt());
            sys.push_str("\n\n");
            if self.config.provider_id == "deepseek" {
                sys.push_str(deepx_config::prompt::DSML_SCHEMA);
            }
        }
        let annotations: Vec<String> = Vec::new();
        self.msg.build_context_for_gate(&sys, &annotations)
    }

    pub fn maybe_save_session(&mut self) {
        if self.msg.has_pending_tools() { return; }
        self.msg.snapshot(&self.config.model, &self.config.reasoning_effort);
    }
}
