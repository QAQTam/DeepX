#[derive(Clone, Copy, PartialEq)]
pub enum Lang {
    En,
    Zh,
}

impl Lang {
    pub fn as_str(&self) -> &str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh",
        }
    }

    pub fn t_setup_welcome(&self) -> &str {
        match self {
            Lang::En => "⚡ DeepX Setup",
            Lang::Zh => "⚡ DeepX 设置",
        }
    }

    pub fn t_select_lang(&self) -> &str {
        match self {
            Lang::En => "Select Language / 选择语言",
            Lang::Zh => "选择语言 / Select Language",
        }
    }

    pub fn t_api_key(&self) -> &str {
        match self {
            Lang::En => "API Key",
            Lang::Zh => "API 密钥",
        }
    }

    pub fn t_enter_key(&self) -> &str {
        match self {
            Lang::En => "Enter your DeepSeek API key from platform.deepseek.com/api_keys",
            Lang::Zh => "输入你的 DeepSeek API 密钥",
        }
    }

    pub fn t_validating(&self) -> &str {
        match self {
            Lang::En => "Validating...",
            Lang::Zh => "验证中...",
        }
    }

    pub fn t_key_valid(&self) -> &str {
        match self {
            Lang::En => "Valid key",
            Lang::Zh => "密钥有效",
        }
    }

    pub fn t_key_invalid(&self) -> &str {
        match self {
            Lang::En => "Invalid key or network error",
            Lang::Zh => "密钥无效或网络错误",
        }
    }

    pub fn t_model(&self) -> &str {
        match self {
            Lang::En => "Model",
            Lang::Zh => "模型",
        }
    }

    pub fn t_select_model(&self) -> &str {
        match self {
            Lang::En => "Select model (or type custom name)",
            Lang::Zh => "选择模型（或输入自定义名称）",
        }
    }

    pub fn t_context_limit(&self) -> &str {
        match self {
            Lang::En => "Context Limit",
            Lang::Zh => "上下文限制",
        }
    }

    pub fn t_max_tokens_desc(&self) -> &str {
        match self {
            Lang::En => "Max context tokens (1,000,000 recommended)",
            Lang::Zh => "最大上下文 Token（推荐 1,000,000）",
        }
    }

    pub fn t_enter_next(&self) -> &str {
        match self {
            Lang::En => "next",
            Lang::Zh => "下一步",
        }
    }

    pub fn t_esc_clear(&self) -> &str {
        match self {
            Lang::En => "clear",
            Lang::Zh => "清空",
        }
    }

    pub fn t_ctrl_c_quit(&self) -> &str {
        match self {
            Lang::En => "quit",
            Lang::Zh => "退出",
        }
    }

    pub fn t_retry(&self) -> &str {
        match self {
            Lang::En => "retry",
            Lang::Zh => "重试",
        }
    }

    // ── Menu / Settings ──

    pub fn t_menu_title(&self) -> &str {
        match self {
            Lang::En => " Settings Menu (F10) ",
            Lang::Zh => " 设置菜单 (F10) ",
        }
    }

    pub fn t_menu_agent_behavior(&self) -> &str {
        match self {
            Lang::En => "── Agent Behavior ──",
            Lang::Zh => "── Agent 行为 ──",
        }
    }
    pub fn t_menu_auto_mode(&self) -> &str {
        match self {
            Lang::En => "Auto Mode",
            Lang::Zh => "自动模式",
        }
    }
    pub fn t_menu_reasoning_effort(&self) -> &str {
        match self {
            Lang::En => "Reasoning Effort",
            Lang::Zh => "推理强度",
        }
    }
    pub fn t_menu_max_tool_rounds(&self) -> &str {
        match self {
            Lang::En => "Max Tool Rounds",
            Lang::Zh => "最大工具轮数",
        }
    }
    pub fn t_menu_model_section(&self) -> &str {
        match self {
            Lang::En => "── Model ──",
            Lang::Zh => "── 模型 ──",
        }
    }
    pub fn t_menu_model(&self) -> &str {
        match self {
            Lang::En => "Model",
            Lang::Zh => "模型",
        }
    }
    pub fn t_menu_max_tokens(&self) -> &str {
        match self {
            Lang::En => "Max Tokens",
            Lang::Zh => "最大 Token",
        }
    }
    pub fn t_menu_context_limit(&self) -> &str {
        match self {
            Lang::En => "Context Limit",
            Lang::Zh => "上下文限制",
        }
    }
    pub fn t_menu_profiles(&self) -> &str {
        match self {
            Lang::En => "── Profiles ──",
            Lang::Zh => "── 配置方案 ──",
        }
    }
    pub fn t_menu_phase_configs(&self) -> &str {
        match self {
            Lang::En => "── Phase Configs ──",
            Lang::Zh => "── 阶段配置 ──",
        }
    }
    pub fn t_menu_phase(&self, phase: &str) -> String {
        match self {
            Lang::En => format!("Phase: {}", phase),
            Lang::Zh => format!("阶段: {}",
                match phase { "plan" => "规划", "coding" => "编码", "debug" => "调试", _ => phase }),
        }
    }
    pub fn t_menu_api(&self) -> &str {
        match self {
            Lang::En => "── API ──",
            Lang::Zh => "── API ──",
        }
    }
    pub fn t_menu_api_key(&self) -> &str {
        match self {
            Lang::En => "API Key",
            Lang::Zh => "API 密钥",
        }
    }
    pub fn t_menu_base_url(&self) -> &str {
        match self {
            Lang::En => "Base URL",
            Lang::Zh => "Base URL",
        }
    }
    pub fn t_menu_interface(&self) -> &str {
        match self {
            Lang::En => "── Interface ──",
            Lang::Zh => "── 界面 ──",
        }
    }
    pub fn t_menu_language(&self) -> &str {
        match self {
            Lang::En => "Language",
            Lang::Zh => "语言",
        }
    }
    pub fn t_menu_nav(&self) -> &str {
        match self { Lang::En => "↑↓ navigate", Lang::Zh => "↑↓ 导航" }
    }
    pub fn t_menu_toggle_edit(&self) -> &str {
        match self { Lang::En => "Enter toggle/edit", Lang::Zh => "Enter 切换/编辑" }
    }
    pub fn t_menu_back(&self) -> &str {
        match self { Lang::En => "Esc back", Lang::Zh => "Esc 返回" }
    }
    pub fn t_menu_close(&self) -> &str {
        match self { Lang::En => " close  ", Lang::Zh => " 关闭  " }
    }
    pub fn t_menu_back_label(&self) -> &str {
        match self { Lang::En => " back", Lang::Zh => " 返回" }
    }
    pub fn t_menu_saved(&self) -> &str {
        match self {
            Lang::En => "Config saved.",
            Lang::Zh => "配置已保存。",
        }
    }
    pub fn t_menu_save_failed(&self) -> &str {
        match self {
            Lang::En => "Failed to save config.",
            Lang::Zh => "保存配置失败。",
        }
    }
    pub fn t_menu_profile_switched(&self, name: &str) -> String {
        match self {
            Lang::En => format!("Switched to profile '{}' (saved, restart to apply)", name),
            Lang::Zh => format!("已切换到方案 '{}'（已保存，重启生效）", name),
        }
    }

    // ── Chat header ──

    pub fn t_chat_phase(&self) -> &str {
        match self { Lang::En => "Phase", Lang::Zh => "阶段" }
    }
    pub fn t_chat_tokens(&self) -> &str {
        match self { Lang::En => "Tokens", Lang::Zh => "Token" }
    }
    pub fn t_chat_hit(&self) -> &str {
        match self { Lang::En => "hit", Lang::Zh => "命中" }
    }
    pub fn t_chat_miss(&self) -> &str {
        match self { Lang::En => "miss", Lang::Zh => "未命中" }
    }

    // ── Chat roles ──

    pub fn t_chat_you(&self) -> &str {
        match self { Lang::En => "You", Lang::Zh => "你" }
    }
    pub fn t_chat_think(&self) -> &str {
        match self { Lang::En => "Think", Lang::Zh => "思考" }
    }

    // ── Chat status ──

    pub fn t_chat_ready(&self) -> &str {
        match self { Lang::En => "Ready", Lang::Zh => "就绪" }
    }
    pub fn t_chat_thinking(&self) -> &str {
        match self { Lang::En => "Thinking...", Lang::Zh => "思考中..." }
    }
    pub fn t_chat_cancelled(&self) -> &str {
        match self { Lang::En => "Cancelled", Lang::Zh => "已取消" }
    }
    pub fn t_chat_error(&self) -> &str {
        match self { Lang::En => "Error", Lang::Zh => "错误" }
    }

    // ── Chat block titles ──

    pub fn t_chat_title(&self) -> &str {
        match self { Lang::En => " Chat ", Lang::Zh => " 对话 " }
    }
    pub fn t_chat_input_title(&self) -> &str {
        match self {
            Lang::En => " Input (Enter: send, Esc: cancel, Ctrl-C: quit) ",
            Lang::Zh => " 输入 (Enter: 发送, Esc: 取消, Ctrl-C: 退出) ",
        }
    }
    pub fn t_chat_input_placeholder(&self) -> &str {
        match self { Lang::En => "Type a message...", Lang::Zh => "输入消息..." }
    }

    // ── Debug overlay ──

    pub fn t_debug_title(&self) -> &str {
        match self { Lang::En => " Debug (F12) ", Lang::Zh => " 调试 (F12) " }
    }
    pub fn t_debug_hp(&self) -> &str {
        match self { Lang::En => "HP", Lang::Zh => "HP" }
    }
    pub fn t_debug_stream(&self) -> &str {
        match self { Lang::En => "Stream", Lang::Zh => "流" }
    }
    pub fn t_debug_session(&self) -> &str {
        match self { Lang::En => "Session", Lang::Zh => "会话" }
    }
    pub fn t_debug_phase(&self) -> &str {
        match self { Lang::En => "Phase", Lang::Zh => "阶段" }
    }
    pub fn t_debug_context(&self) -> &str {
        match self { Lang::En => "Context", Lang::Zh => "上下文" }
    }
    pub fn t_debug_tools(&self) -> &str {
        match self { Lang::En => "Tools", Lang::Zh => "工具" }
    }
    pub fn t_debug_calls(&self) -> &str {
        match self { Lang::En => "calls", Lang::Zh => "次调用" }
    }
    pub fn t_debug_fail(&self) -> &str {
        match self { Lang::En => "fail", Lang::Zh => "失败" }
    }

    // ── Ask popup ──

    pub fn t_ask_title(&self) -> &str {
        match self { Lang::En => " Ask ", Lang::Zh => " 询问 " }
    }
    pub fn t_ask_other(&self) -> &str {
        match self { Lang::En => "Other", Lang::Zh => "其他" }
    }
    pub fn t_ask_other_placeholder(&self) -> &str {
        match self { Lang::En => "Other (______)", Lang::Zh => "其他 (______)" }
    }
    pub fn t_ask_help(&self) -> &str {
        match self { Lang::En => " ↑↓ select  Enter confirm  Esc cancel", Lang::Zh => " ↑↓ 选择  Enter 确认  Esc 取消" }
    }

    // ── Sessions screen ──

    pub fn t_session_title(&self) -> &str {
        match self { Lang::En => " Sessions — Select or start new ", Lang::Zh => " 会话 — 选择或新建 " }
    }
    pub fn t_session_new(&self) -> &str {
        match self { Lang::En => "+ New Session", Lang::Zh => "+ 新会话" }
    }
    pub fn t_session_msgs(&self) -> &str {
        match self { Lang::En => "msgs", Lang::Zh => "消息" }
    }
    pub fn t_session_select_hint(&self) -> &str {
        match self { Lang::En => " select  ", Lang::Zh => " 选择  " }
    }
    pub fn t_session_resume_hint(&self) -> &str {
        match self { Lang::En => " resume/new  ", Lang::Zh => " 恢复/新建  " }
    }
    pub fn t_session_quit_hint(&self) -> &str {
        match self { Lang::En => " quit", Lang::Zh => " 退出" }
    }

    // ── Setup wizard ──

    pub fn t_setup_lang_en_name(&self) -> &str {
        match self { Lang::En => "English", Lang::Zh => "English" }
    }
    pub fn t_setup_lang_en_desc(&self) -> &str {
        match self { Lang::En => "Use English throughout the interface", Lang::Zh => "界面使用英文" }
    }
    pub fn t_setup_lang_zh_name(&self) -> &str {
        match self { Lang::En => "中文", Lang::Zh => "中文" }
    }
    pub fn t_setup_lang_zh_desc(&self) -> &str {
        match self { Lang::En => "界面和对话使用中文", Lang::Zh => "界面和对话使用中文" }
    }
    pub fn t_setup_nav_hint(&self) -> &str {
        match self { Lang::En => "  ↑↓ to change, Enter to confirm", Lang::Zh => "  ↑↓ 选择, Enter 确认" }
    }
    pub fn t_setup_tokens_unit(&self) -> &str {
        match self { Lang::En => "  tokens", Lang::Zh => "  tokens" }
    }
    pub fn t_setup_model_required(&self) -> &str {
        match self { Lang::En => "Model name is required", Lang::Zh => "模型名称必填" }
    }
    pub fn t_setup_context_min(&self) -> &str {
        match self { Lang::En => "Context limit must be at least 1024", Lang::Zh => "上下文限制至少 1024" }
    }
    pub fn t_setup_invalid_number(&self) -> &str {
        match self { Lang::En => "Invalid number", Lang::Zh => "无效数字" }
    }

    // ── General status ──

    pub fn t_failed_agent(&self) -> &str {
        match self { Lang::En => "Failed to start agent", Lang::Zh => "启动 Agent 失败" }
    }
    pub fn t_config_saved(&self) -> &str {
        match self { Lang::En => "Config saved to", Lang::Zh => "配置已保存到" }
    }
    pub fn t_session_restored(&self, seed: &str, msgs: u64, tokens: u32) -> String {
        match self {
            Lang::En => format!("Session {} restored ({} msgs, {} tokens)", seed, msgs, tokens),
            Lang::Zh => format!("会话 {} 已恢复 ({} 条消息, {} token)", seed, msgs, tokens),
        }
    }
    pub fn t_cache_warn_low(&self) -> &str {
        match self {
            Lang::En => "Cache hit rate critically low, consider investigating",
            Lang::Zh => "⚠ 缓存命中持续过低，建议暂停并排查",
        }
    }
    pub fn t_cache_warn_moderate(&self) -> &str {
        match self {
            Lang::En => "Cache hit rate below average",
            Lang::Zh => "缓存命中偏低",
        }
    }

    // ── Tool call status labels ──

    pub fn t_tool_exploring(&self) -> &str {
        match self { Lang::En => "Exploring", Lang::Zh => "正在探索" }
    }
    pub fn t_tool_reading(&self) -> &str {
        match self { Lang::En => "Reading", Lang::Zh => "正在读取" }
    }
    pub fn t_tool_writing(&self) -> &str {
        match self { Lang::En => "Writing", Lang::Zh => "正在写入" }
    }
    pub fn t_tool_searching(&self) -> &str {
        match self { Lang::En => "Searching", Lang::Zh => "正在搜索" }
    }
    pub fn t_tool_executing(&self) -> &str {
        match self { Lang::En => "Executing", Lang::Zh => "正在执行" }
    }
    pub fn t_tool_deleting(&self) -> &str {
        match self { Lang::En => "Deleting", Lang::Zh => "正在删除" }
    }
    pub fn t_tool_moving(&self) -> &str {
        match self { Lang::En => "Moving", Lang::Zh => "正在移动" }
    }
    pub fn t_tool_copying(&self) -> &str {
        match self { Lang::En => "Copying", Lang::Zh => "正在复制" }
    }
    pub fn t_tool_listing(&self) -> &str {
        match self { Lang::En => "Listing", Lang::Zh => "正在列出" }
    }
    pub fn t_tool_diffing(&self) -> &str {
        match self { Lang::En => "Diffing", Lang::Zh => "正在对比" }
    }
    pub fn t_tool_committing(&self) -> &str {
        match self { Lang::En => "Committing", Lang::Zh => "正在提交" }
    }
    pub fn t_tool_creating(&self) -> &str {
        match self { Lang::En => "Creating", Lang::Zh => "正在创建" }
    }
    pub fn t_tool_updating(&self) -> &str {
        match self { Lang::En => "Updating", Lang::Zh => "正在更新" }
    }
    pub fn t_tool_resolving(&self) -> &str {
        match self { Lang::En => "Resolving", Lang::Zh => "正在解析" }
    }
    pub fn t_tool_querying(&self) -> &str {
        match self { Lang::En => "Querying", Lang::Zh => "正在查询" }
    }
    pub fn t_tool_asking(&self) -> &str {
        match self { Lang::En => "Asking", Lang::Zh => "正在询问" }
    }
    pub fn t_tool_truncated(&self, extra: usize) -> String {
        match self {
            Lang::En => format!("  ... (+{} chars)", extra),
            Lang::Zh => format!("  ... (+{} 字符)", extra),
        }
    }
    pub fn t_tool_lines_total(&self, total: usize, shown: usize) -> String {
        match self {
            Lang::En => format!("  ... {} lines total (showing first {})", total, shown),
            Lang::Zh => format!("  ... 共 {} 行 (显示前 {})", total, shown),
        }
    }
}
