//! Settings view: API config, limits, language.

use crate::app::AppState;
use egui::{Color32, RichText, ScrollArea};

const ACCENT: Color32 = Color32::from_rgb(0xD4, 0x78, 0x3C);
const TEXT: Color32 = Color32::from_rgb(0x2C, 0x24, 0x16);
const MUTED: Color32 = Color32::from_rgb(0x9B, 0x8D, 0x7A);

/// Settings form state, persisted in egui temp data.
#[derive(Clone)]
struct SettingsForm {
    api_key: String,
    provider_id: String,
    endpoint_id: String,
    model: String,
    base_url: String,
    max_tokens: u32,
    context_limit: u32,
    reasoning_effort: String,
    lang: String,
    saved_toast: bool,
    loaded: bool,
}

impl Default for SettingsForm {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            provider_id: "deepseek".into(),
            endpoint_id: "openai".into(),
            model: String::new(),
            base_url: String::new(),
            max_tokens: 16384,
            context_limit: 1_000_000,
            reasoning_effort: "high".into(),
            lang: "en".into(),
            saved_toast: false,
            loaded: false,
        }
    }
}

impl SettingsForm {
    fn load(&mut self) {
        if self.loaded {
            return;
        }
        if let Ok(cfg) = deepx_config::Config::load() {
            self.api_key = cfg.api_key;
            self.provider_id = cfg.provider_id;
            self.endpoint_id = cfg.endpoint;
            self.model = cfg.model;
            self.base_url = cfg.base_url;
            self.max_tokens = cfg.max_tokens;
            self.context_limit = cfg.context_limit;
            self.reasoning_effort = cfg.reasoning_effort;
            self.lang = cfg.lang.unwrap_or_else(|| "en".into());
        }
        self.loaded = true;
    }

    fn save(&mut self, s: &mut AppState) {
        let mut cfg = deepx_config::Config::load().unwrap_or_default();
        if !self.api_key.is_empty() {
            cfg.api_key = self.api_key.clone();
        }
        cfg.provider_id = self.provider_id.clone();
        cfg.endpoint = self.endpoint_id.clone();
        cfg.model = self.model.clone();
        cfg.base_url = self.base_url.clone();
        cfg.max_tokens = self.max_tokens;
        cfg.context_limit = self.context_limit;
        cfg.reasoning_effort = self.reasoning_effort.clone();
        cfg.lang = Some(self.lang.clone());
        if let Err(e) = cfg.save() {
            s.messages.push_back(crate::app::Message::system(&format!(
                "配置保存失败: {e}"
            )));
        } else {
            self.saved_toast = true;
            s.send_raw(deepx_proto::Ui2Agent::ReloadConfig);
        }
    }

    fn on_provider_change(&mut self, pid: &str) {
        self.provider_id = pid.to_string();
        if let Some(ep) =
            deepx_config::registry::first_endpoint_for(pid)
        {
            self.endpoint_id = ep.id.clone();
            self.base_url = ep.base_url.clone();
            self.model = ep.default_model.clone();
        }
    }

    fn on_endpoint_change(&mut self, eid: &str) {
        self.endpoint_id = eid.to_string();
        if let Some(ep) = deepx_config::registry::find_endpoint(
            &self.provider_id, eid,
        ) {
            self.base_url = ep.base_url.clone();
            self.model = ep.default_model.clone();
        }
    }
}

pub(crate) fn render_settings(ui: &mut egui::Ui, s: &mut AppState) {
    let id = egui::Id::new("settings_form");
    let mut form = ui
        .ctx()
        .data_mut(|d| d.get_temp::<SettingsForm>(id).unwrap_or_default());
    form.load();

    let mut close = false;

    // Header
    ui.horizontal(|ui| {
        ui.heading(RichText::new("设置").color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(RichText::new("✕").size(14.0).color(MUTED))
                .clicked()
            {
                close = true;
            }
        });
    });
    ui.separator();
    ui.add_space(8.0);

    ScrollArea::vertical().show(ui, |ui| {
        // ── API Section ──
        section_header(ui, "API");

        // Provider
        ui.label(RichText::new("Provider").size(12.0).color(MUTED));
        let providers = deepx_config::registry::all_providers();
        egui::ComboBox::from_id_salt("provider")
            .selected_text(
                providers
                    .iter()
                    .find(|p| p.id == form.provider_id)
                    .map(|p| p.display.as_str())
                    .unwrap_or(&form.provider_id),
            )
            .show_ui(ui, |ui| {
                for p in &providers {
                    if ui
                        .selectable_label(
                            form.provider_id == p.id,
                            RichText::new(&p.display).size(12.0),
                        )
                        .clicked()
                    {
                        form.on_provider_change(&p.id);
                    }
                }
            });
        ui.add_space(4.0);

        // Endpoint
        ui.label(RichText::new("Endpoint").size(12.0).color(MUTED));
        let current_provider = providers
            .iter()
            .find(|p| p.id == form.provider_id);
        let endpoints: Vec<&deepx_types::EndpointSpec> = current_provider
            .map(|p| p.endpoints.iter().collect())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("endpoint")
            .selected_text(
                endpoints
                    .iter()
                    .find(|e| e.id == form.endpoint_id)
                    .map(|e| e.display.as_str())
                    .unwrap_or(&form.endpoint_id),
            )
            .show_ui(ui, |ui| {
                for ep in &endpoints {
                    if ui
                        .selectable_label(
                            form.endpoint_id == ep.id,
                            RichText::new(&ep.display).size(12.0),
                        )
                        .clicked()
                    {
                        form.on_endpoint_change(&ep.id);
                    }
                }
            });
        ui.add_space(8.0);

        // API Key
        ui.label(RichText::new("API Key").size(12.0).color(MUTED));
        let mut key_visible = false;
        ui.horizontal(|ui| {
            let pw = egui::TextEdit::singleline(&mut form.api_key).password(!key_visible);
            ui.add_sized([ui.available_width() - 24.0, 20.0], pw);
            if ui
                .small_button(if key_visible { "🙈" } else { "👁" })
                .clicked()
            {
                key_visible = !key_visible;
            }
        });
        ui.add_space(8.0);

        // Model
        ui.label(RichText::new("Model").size(12.0).color(MUTED));
        let models: Vec<String> = current_provider
            .and_then(|p| p.endpoints.iter().find(|e| e.id == form.endpoint_id))
            .map(|ep| ep.models.clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("model")
                .selected_text(&form.model)
                .show_ui(ui, |ui| {
                    for m in &models {
                        if ui
                            .selectable_label(
                                form.model == *m,
                                RichText::new(m).size(12.0),
                            )
                            .clicked()
                        {
                            form.model = m.clone();
                        }
                    }
                });
            ui.label(RichText::new("or custom:").size(11.0).color(MUTED));
            ui.add_sized(
                [160.0, 20.0],
                egui::TextEdit::singleline(&mut form.model)
                    .hint_text("custom model name"),
            );
        });
        ui.add_space(4.0);

        // Base URL
        ui.label(RichText::new("Base URL").size(12.0).color(MUTED));
        ui.add_sized(
            [ui.available_width(), 20.0],
            egui::TextEdit::singleline(&mut form.base_url),
        );
        ui.add_space(12.0);

        // ── Limits Section ──
        section_header(ui, "Limits");

        ui.label(RichText::new("Max Tokens").size(12.0).color(MUTED));
        ui.add(
            egui::Slider::new(&mut form.max_tokens, 1024..=131072)
                .step_by(1024.0)
                .text("tokens"),
        );
        ui.label(
            RichText::new(format!("{} tokens", form.max_tokens))
                .size(11.0)
                .color(MUTED),
        );
        ui.add_space(4.0);

        ui.label(RichText::new("Context Limit").size(12.0).color(MUTED));
        ui.add(
            egui::Slider::new(&mut form.context_limit, 100_000..=10_000_000)
                .step_by(100_000.0)
                .text("ctx"),
        );
        ui.label(
            RichText::new(fmt_tokens(form.context_limit as u64))
                .size(11.0)
                .color(MUTED),
        );
        ui.add_space(4.0);

        // Reasoning Effort
        ui.label(
            RichText::new("Reasoning Effort").size(12.0).color(MUTED),
        );
        egui::ComboBox::from_id_salt("effort")
            .selected_text(&form.reasoning_effort)
            .show_ui(ui, |ui| {
                for e in &["high", "max", "medium", "low"] {
                    if ui
                        .selectable_label(
                            form.reasoning_effort == *e,
                            RichText::new(*e).size(12.0),
                        )
                        .clicked()
                    {
                        form.reasoning_effort = e.to_string();
                    }
                }
            });
        ui.add_space(12.0);

        // ── Language Section ──
        section_header(ui, "Language");

        egui::ComboBox::from_id_salt("lang")
            .selected_text(if form.lang == "zh" { "中文" } else { "English" })
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(
                        form.lang == "en",
                        RichText::new("English").size(12.0),
                    )
                    .clicked()
                {
                    form.lang = "en".into();
                }
                if ui
                    .selectable_label(
                        form.lang == "zh",
                        RichText::new("中文").size(12.0),
                    )
                    .clicked()
                {
                    form.lang = "zh".into();
                }
            });
        ui.add_space(16.0);

        // ── Save ──
        ui.horizontal(|ui| {
            let btn = egui::Button::new(
                RichText::new(if form.saved_toast { "✓ 已保存" } else { "保存配置" })
                    .size(14.0),
            )
            .fill(if form.saved_toast {
                Color32::from_rgb(0xE8, 0xF5, 0xE9)
            } else {
                ACCENT
            });
            if ui.add_sized([120.0, 28.0], btn).clicked() {
                form.save(s);
            }
            if ui
                .button(RichText::new("返回聊天").size(14.0))
                .clicked()
            {
                close = true;
            }
        });
    });

    // Persist form state
    ui.data_mut(|d| d.insert_temp(id, form.clone()));

    if close {
        s.view = crate::app::View::Chat;
    }
}

fn section_header(ui: &mut egui::Ui, label: &str) {
    ui.add_space(4.0);
    ui.colored_label(
        ACCENT,
        RichText::new(label).size(14.0).strong(),
    );
    ui.add_space(4.0);
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
