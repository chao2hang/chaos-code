//! Provider modal state — multi-step form for `/provider` subcommands.
//!
//! Hub UX (OpenCode-style): bare `/provider` opens a selectable channel list.
//! Enter drills into a per-channel action menu; Esc walks back a level.

use crate::views::modal_window::ModalWindowState;

/// 公开显示标题。
pub const MODAL_TITLE: &str = "渠道管理";

/// 认证方式选项。
pub const AUTH_SCHEMES: &[&str] = &["bearer", "x_api_key"];

/// API 后端选项。
pub const API_BACKENDS: &[&str] = &["responses", "chat_completions", "messages"];

/// 渠道操作菜单项（二级菜单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAction {
    SetKey,
    Models,
    SetModel,
    Refresh,
}

impl ProviderAction {
    pub const ALL: &[ProviderAction] = &[
        ProviderAction::SetKey,
        ProviderAction::Models,
        ProviderAction::SetModel,
        ProviderAction::Refresh,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::SetKey => "设置 API Key",
            Self::Models => "查看可用模型",
            Self::SetModel => "切换模型",
            Self::Refresh => "刷新模型列表",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::SetKey => "写入/更新密钥",
            Self::Models => "从渠道拉取模型",
            Self::SetModel => "选择并设为当前模型",
            Self::Refresh => "重新拉取模型列表",
        }
    }
}

/// 预设渠道模板。
pub struct ProviderPreset {
    pub name: &'static str,
    pub display: &'static str,
    pub base_url: &'static str,
    pub auth_scheme: &'static str,
    pub api_backend: &'static str,
}

/// 内置预设渠道列表。
pub const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "openai",
        display: "OpenAI",
        base_url: "https://api.openai.com/v1",
        auth_scheme: "bearer",
        api_backend: "chat_completions",
    },
    ProviderPreset {
        name: "anthropic",
        display: "Anthropic (Claude)",
        base_url: "https://api.anthropic.com",
        auth_scheme: "x_api_key",
        api_backend: "messages",
    },
    ProviderPreset {
        name: "deepseek",
        display: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        auth_scheme: "bearer",
        api_backend: "chat_completions",
    },
    ProviderPreset {
        name: "xai",
        display: "xAI (Grok)",
        base_url: "https://api.x.ai/v1",
        auth_scheme: "bearer",
        api_backend: "chat_completions",
    },
];

/// 多步表单的当前步骤（仅 `Add` 模式使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormStep {
    /// 预设选择（第一步）。
    Preset,
    /// 渠道名称（仅自定义预设时出现）。
    Name,
    BaseUrl,
    AuthScheme,
    ApiBackend,
    ApiKey,
}

impl FormStep {
    /// 步骤的中文显示名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::Preset => "选择预设",
            Self::Name => "渠道名称",
            Self::BaseUrl => "Base URL",
            Self::AuthScheme => "认证方式",
            Self::ApiBackend => "API 后端",
            Self::ApiKey => "API Key",
        }
    }
}

/// 模态框的模式——对应 `/provider` 的子命令 / hub 导航。
#[derive(Debug, Clone)]
pub enum ProviderModalMode {
    /// 渠道列表 hub（裸 `/provider` 或 `/provider list`）。
    List,
    /// 某渠道的操作二级菜单。
    Actions(String),
    /// `/provider add` — 多步表单：name → base_url → auth_scheme → api_backend → api_key。
    Add,
    /// `/provider set-key <name>` — 单步输入 API Key。
    SetKey(String),
    /// `/provider models <name>` — 展示渠道可用模型列表。
    Models(String),
    /// `/provider set-model <name>` — 选择并设置当前模型。
    SetModel(String),
}

/// 输入事件的输出。
#[derive(Debug)]
pub enum ProviderKeyOutcome {
    Close,
    Changed,
    Unchanged,
    /// 表单确认提交（仅 Enter 在可提交字段上触发）。
    Commit,
    /// 切换到指定模型（触发 `Action::SetDefaultModel`）。
    SwitchModel(String),
}

/// Provider 模态框状态。
pub struct ProviderModalState {
    pub window: ModalWindowState,
    pub mode: ProviderModalMode,
    /// 是否从 hub 进入（裸 `/provider` / 列表导航）。为 true 时 Esc 逐级返回而非直接关闭。
    pub from_hub: bool,
    // ── Add 模式的表单字段 ──
    pub name: String,
    pub base_url: String,
    pub auth_scheme_idx: usize,
    pub api_backend_idx: usize,
    pub api_key: String,
    pub current_step: FormStep,
    // ── 通用状态 ──
    /// 错误消息（红色显示）。
    pub error: Option<String>,
    /// 成功消息（绿色显示，按任意键后清除）。
    pub success: Option<String>,
    /// `Models` / `SetModel` 模式下从 API 获取的模型列表。
    pub models: Vec<String>,
    /// 模型列表是否正在加载。
    pub models_loading: bool,
    /// `List` / `Actions` / `Models` / `SetModel` 模式下的选中索引。
    pub selected: usize,
    /// 垂直滚动偏移。
    pub scroll_offset: usize,
    /// `List` 模式下读取的渠道条目。
    pub providers: Vec<ProviderSummary>,
}

/// `/provider list` 显示的一行渠道摘要。
#[derive(Debug, Clone)]
pub struct ProviderSummary {
    pub name: String,
    pub base_url: String,
    pub auth_scheme: String,
    pub api_backend: String,
    pub has_key: bool,
    pub is_current: bool,
}

impl ProviderModalState {
    /// 构造指定模式的新状态。
    pub fn new(mode: ProviderModalMode) -> Self {
        let from_hub = matches!(mode, ProviderModalMode::List);
        Self {
            window: ModalWindowState::new(),
            mode,
            from_hub,
            name: String::new(),
            base_url: String::new(),
            auth_scheme_idx: 0,
            api_backend_idx: 1, // 默认 chat_completions
            api_key: String::new(),
            current_step: FormStep::Preset,
            error: None,
            success: None,
            models: Vec::new(),
            models_loading: false,
            selected: 0,
            scroll_offset: 0,
            providers: Vec::new(),
        }
    }

    /// 当前认证方式。
    pub fn auth_scheme(&self) -> &'static str {
        AUTH_SCHEMES[self.auth_scheme_idx.min(AUTH_SCHEMES.len() - 1)]
    }

    /// 当前 API 后端。
    pub fn api_backend(&self) -> &'static str {
        API_BACKENDS[self.api_backend_idx.min(API_BACKENDS.len() - 1)]
    }

    /// 清除错误和成功消息。
    pub fn clear_messages(&mut self) {
        self.error = None;
        self.success = None;
    }

    /// 列表总行数：渠道 + 「+ 添加渠道」。
    pub fn list_row_count(&self) -> usize {
        self.providers.len() + 1
    }

    /// 当前选中是否为「添加渠道」行。
    pub fn list_add_selected(&self) -> bool {
        self.selected >= self.providers.len()
    }

    /// 重新加载渠道列表（hub 返回时）。
    pub fn reload_providers(&mut self) {
        self.error = None;
        match crate::slash::commands::provider::load_config() {
            Ok(doc) => {
                let providers = crate::slash::commands::provider::list_providers(&doc);
                let current_provider =
                    crate::slash::commands::provider::current_provider_name(&doc);
                self.providers = providers
                    .iter()
                    .map(|name| {
                        let base_url = crate::slash::commands::provider::provider_field(
                            &doc, name, "base_url",
                        )
                        .unwrap_or_default();
                        let auth_scheme = crate::slash::commands::provider::provider_field(
                            &doc, name, "auth_scheme",
                        )
                        .unwrap_or_default();
                        let api_backend = crate::slash::commands::provider::provider_field(
                            &doc, name, "api_backend",
                        )
                        .unwrap_or_default();
                        let has_key = crate::slash::commands::provider::provider_field(
                            &doc, name, "api_key",
                        )
                        .is_some();
                        let is_current = current_provider.as_deref() == Some(name.as_str());
                        ProviderSummary {
                            name: name.clone(),
                            base_url,
                            auth_scheme,
                            api_backend,
                            has_key,
                            is_current,
                        }
                    })
                    .collect();
            }
            Err(e) => {
                self.error = Some(e);
                self.providers.clear();
            }
        }
    }

    /// 拉取并填充模型列表。
    pub fn load_models_for(&mut self, name: &str) {
        self.models_loading = true;
        self.models.clear();
        self.error = None;
        self.selected = 0;
        self.scroll_offset = 0;
        match crate::slash::commands::provider::fetch_provider_models(name) {
            Ok(models) => {
                self.models = models;
                self.models_loading = false;
            }
            Err(e) => {
                self.error = Some(e);
                self.models_loading = false;
            }
        }
    }

    /// 导航回渠道列表 hub。
    pub fn go_list(&mut self) {
        self.from_hub = true;
        self.mode = ProviderModalMode::List;
        self.clear_messages();
        self.api_key.clear();
        self.models.clear();
        self.models_loading = false;
        self.current_step = FormStep::Preset;
        self.selected = self.selected.min(self.list_row_count().saturating_sub(1));
        self.reload_providers();
        let max = self.list_row_count().saturating_sub(1);
        if self.selected > max {
            self.selected = max;
        }
    }

    /// 打开某渠道的操作菜单。
    pub fn go_actions(&mut self, name: String) {
        self.from_hub = true;
        self.mode = ProviderModalMode::Actions(name);
        self.clear_messages();
        self.api_key.clear();
        self.models.clear();
        self.models_loading = false;
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// 打开添加表单（保留 from_hub）。
    pub fn go_add(&mut self) {
        self.mode = ProviderModalMode::Add;
        self.clear_messages();
        self.name.clear();
        self.base_url.clear();
        self.api_key.clear();
        self.auth_scheme_idx = 0;
        self.api_backend_idx = 1;
        self.current_step = FormStep::Preset;
        self.selected = 0;
    }

    /// 打开设 Key（保留 from_hub）。
    pub fn go_set_key(&mut self, name: String) {
        self.mode = ProviderModalMode::SetKey(name);
        self.clear_messages();
        self.api_key.clear();
        self.selected = 0;
    }

    /// 打开模型列表（查看）。
    pub fn go_models(&mut self, name: String) {
        self.mode = ProviderModalMode::Models(name.clone());
        self.clear_messages();
        self.load_models_for(&name);
    }

    /// 打开切换模型。
    pub fn go_set_model(&mut self, name: String) {
        self.mode = ProviderModalMode::SetModel(name.clone());
        self.clear_messages();
        self.load_models_for(&name);
    }

    /// Esc 逐级返回；返回 true 表示应关闭模态框。
    pub fn navigate_back(&mut self) -> bool {
        if self.success.is_some() {
            self.success = None;
        }
        match self.mode.clone() {
            ProviderModalMode::List => true,
            ProviderModalMode::Actions(name) => {
                let keep = self
                    .providers
                    .iter()
                    .position(|p| p.name == name)
                    .unwrap_or(0);
                self.go_list();
                if keep < self.list_row_count() {
                    self.selected = keep;
                }
                false
            }
            ProviderModalMode::Add => {
                if !self.from_hub {
                    return true;
                }
                if self.current_step != FormStep::Preset {
                    let from_preset = PROVIDER_PRESETS.iter().any(|p| p.name == self.name)
                        && !self.base_url.is_empty()
                        && self.current_step == FormStep::ApiKey;
                    self.current_step = if from_preset {
                        FormStep::Preset
                    } else {
                        match self.current_step {
                            FormStep::Name => FormStep::Preset,
                            FormStep::BaseUrl => FormStep::Name,
                            FormStep::AuthScheme => FormStep::BaseUrl,
                            FormStep::ApiBackend => FormStep::AuthScheme,
                            FormStep::ApiKey => FormStep::ApiBackend,
                            FormStep::Preset => FormStep::Preset,
                        }
                    };
                    self.error = None;
                    return false;
                }
                self.go_list();
                false
            }
            ProviderModalMode::SetKey(name)
            | ProviderModalMode::Models(name)
            | ProviderModalMode::SetModel(name) => {
                if self.from_hub {
                    self.go_actions(name);
                    false
                } else {
                    true
                }
            }
        }
    }
}
