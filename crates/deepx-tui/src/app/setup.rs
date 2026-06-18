use deepx_types::{ConfigStore, PersistentConfig};
use crate::i18n::Lang;

// ── Setup wizard state ──

pub struct SetupState {
    pub step: usize,
    pub lang: Lang,
    pub api_key: String,
    pub model: String,
    pub model_list: Vec<String>,
    pub model_index: usize,
    pub context_limit: String,
    pub error: String,
    pub status: String,
    pub models_loaded: bool,
}

impl SetupState {
    pub fn new() -> Self {
        let (pid, ep) = deepx_config::registry::first_provider_endpoint();
        let def_model = deepx_config::registry::default_model_for(&pid, &ep);
        let model_list = deepx_config::registry::find_endpoint(&pid, &ep)
            .map(|e| if e.models.is_empty() { vec![] } else { e.models.clone() })
            .unwrap_or_default();
        let model_index = model_list.iter().position(|m| m == &def_model).unwrap_or(0);
        Self {
            step: 0,
            lang: Lang::En,
            api_key: String::new(),
            model: def_model,
            model_list,
            model_index,
            context_limit: String::from("1000000"),
            error: String::new(),
            status: String::new(),
            models_loaded: false,
        }
    }

    pub fn total_steps(&self) -> usize { 4 }

    pub fn fill_from_store(&mut self, store: &ConfigStore) {
        if let Some(key) = store.load_api_key() {
            self.api_key = key;
        }
        if let Some(v) = store.load_value() {
            if let Some(m) = v.get("model").and_then(|m| m.as_str()) {
                self.model = m.to_string();
                if let Some(idx) = self.model_list.iter().position(|n| n == m) {
                    self.model_index = idx;
                }
            }
            if let Some(c) = v.get("context_limit").and_then(|c| c.as_u64()) {
                self.context_limit = c.to_string();
            }
            if let Some(l) = v.get("lang").and_then(|l| l.as_str()) {
                self.lang = Lang::from_str(l);
            }
        }
    }

    pub fn current_value(&self) -> &str {
        match self.step {
            0 => self.lang.as_str(),
            1 => &self.api_key,
            2 => &self.model,
            3 => &self.context_limit,
            _ => "",
        }
    }

    fn current_value_mut(&mut self) -> &mut String {
        match self.step {
            1 => &mut self.api_key,
            2 => &mut self.model,
            3 => &mut self.context_limit,
            _ => &mut self.error,
        }
    }

    pub fn backspace(&mut self) {
        if self.step >= 1 {
            self.current_value_mut().pop();
        }
    }

    pub fn type_char(&mut self, c: char) {
        if self.step >= 1 {
            self.current_value_mut().push(c);
        }
    }

    pub fn clear_field(&mut self) {
        if self.step >= 1 {
            self.current_value_mut().clear();
        }
    }

    pub fn next(&mut self) -> bool {
        self.validate();
        if !self.error.is_empty() {
            return false;
        }
        self.step += 1;
        self.step >= self.total_steps()
    }

    fn validate(&mut self) {
        self.error.clear();
        match self.step {
            0 => {} // language always valid
            1 => {
                self.api_key = self.api_key.trim().to_string();
                if self.api_key.is_empty() {
                    self.error = self.lang.t_key_invalid().to_string();
                }
            }
            2 => {
                self.model = self.model.trim().to_string();
                if self.model.is_empty() {
                    self.error = self.lang.t_setup_model_required().to_string();
                }
            }
            3 => {
                if let Ok(v) = self.context_limit.parse::<u32>() {
                    if v < 1024 {
                        self.error = self.lang.t_setup_context_min().to_string();
                    }
                } else {
                    self.error = self.lang.t_setup_invalid_number().to_string();
                }
            }
            _ => {}
        }
    }

    /// Validate API key by checking connectivity. Model list already comes from registry presets.
    /// Returns true if the key appears valid (non-empty + connects).
    pub fn fetch_models(&mut self, provider_id: &str) -> bool {
        let key = self.api_key.trim();
        if key.is_empty() {
            return false;
        }
        let ep_id = deepx_config::registry::first_endpoint_for(provider_id)
            .map(|e| e.id).unwrap_or_else(|| "openai".into());
        let url = match deepx_config::registry::models_url_for(provider_id, &ep_id) {
            Some(u) => u,
            None => return false,
        };
        let resp = ureq::get(&url)
            .header("Authorization", &format!("Bearer {}", key))
            .call();
        match resp {
            Ok(_r) => {
                self.models_loaded = true;
                true
            }
            Err(_) => false,
        }
    }

    pub fn cursor_row_offset(&self) -> u16 {
        match self.step {
            0 => 8,
            1 => 6,
            2 => {
                if self.models_loaded {
                    let n = self.model_list.len().min(6);
                    let extra = if self.model_list.len() > 6 { 1 } else { 0 };
                    6 + n as u16 + extra as u16
                } else {
                    3
                }
            }
            3 => 5,
            _ => 8,
        }
    }

    pub fn toggle_lang(&mut self) {
        self.lang = match self.lang {
            Lang::En => Lang::Zh,
            Lang::Zh => Lang::En,
        };
    }

    pub fn to_persistent_config(&self) -> PersistentConfig {
        let (pid, ep) = deepx_config::registry::first_provider_endpoint();
        let base_url = deepx_config::registry::base_url_for(&pid, &ep);
        PersistentConfig {
            api_key: Some(self.api_key.trim().to_string()),
            model: Some(self.model.trim().to_string()),
            base_url: Some(base_url),
            context_limit: Some(self.context_limit.parse().unwrap_or(1_000_000)),
            lang: Some(self.lang.as_str().to_string()),
            ..Default::default()
        }
    }
}
