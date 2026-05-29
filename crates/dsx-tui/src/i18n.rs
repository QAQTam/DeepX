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

    pub fn label(&self) -> &str {
        match self {
            Lang::En => "English",
            Lang::Zh => "中文",
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
            Lang::Zh => "输入你的 DeepSeek API 密钥 (platform.deepseek.com/api_keys)",
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
}
