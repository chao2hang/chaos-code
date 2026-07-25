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
    /// 编辑 base_url / 认证 / 后端 / API Key。
    Edit,
    SetKey,
    Models,
    /// 手动输入模型 ID（不依赖上游 /models 列表）。
    ManualModel,
    /// 为已有/选中模型设置 max_completion_tokens 等参数。
    ConfigureModel,
    SetModel,
    Refresh,
}

impl ProviderAction {
    pub const ALL: &[ProviderAction] = &[
        ProviderAction::Edit,
        ProviderAction::SetKey,
        ProviderAction::Models,
        ProviderAction::ManualModel,
        ProviderAction::ConfigureModel,
        ProviderAction::SetModel,
        ProviderAction::Refresh,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Edit => "编辑渠道",
            Self::SetKey => "设置 API Key",
            Self::Models => "查看可用模型",
            Self::ManualModel => "手动输入模型",
            Self::ConfigureModel => "配置模型参数",
            Self::SetModel => "切换模型",
            Self::Refresh => "刷新模型列表",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Edit => "修改 URL / 认证 / 后端 / 密钥",
            Self::SetKey => "写入/更新密钥",
            Self::Models => "从渠道拉取模型",
            Self::ManualModel => "手写模型 ID 并设为当前",
            Self::ConfigureModel => "max_tokens / 上下文 / temperature",
            Self::SetModel => "选择并设为当前模型",
            Self::Refresh => "重新拉取模型列表",
        }
    }
}

/// 模型采样/窗口参数表单字段（手动添加模型 或 配置已有模型共用）。
///
/// 留空表示不写入（新建）或清除覆盖（配置已有），回退到渠道/全局默认。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelParamField {
    MaxCompletionTokens,
    ContextWindow,
    Temperature,
    TopP,
}

impl ModelParamField {
    pub const ALL: &[ModelParamField] = &[
        ModelParamField::MaxCompletionTokens,
        ModelParamField::ContextWindow,
        ModelParamField::Temperature,
        ModelParamField::TopP,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::MaxCompletionTokens => "max_completion_tokens",
            Self::ContextWindow => "context_window",
            Self::Temperature => "temperature",
            Self::TopP => "top_p",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::MaxCompletionTokens => "单次回复上限，如 8192 / 16384（留空=默认）",
            Self::ContextWindow => "上下文窗口，如 128000 / 200000（留空=默认）",
            Self::Temperature => "采样温度 0–2，如 0.7（留空=默认）",
            Self::TopP => "nucleus top_p 0–1，如 0.95（留空=默认）",
        }
    }

    pub fn next(self) -> Option<Self> {
        match self {
            Self::MaxCompletionTokens => Some(Self::ContextWindow),
            Self::ContextWindow => Some(Self::Temperature),
            Self::Temperature => Some(Self::TopP),
            Self::TopP => None,
        }
    }

    pub fn prev(self) -> Option<Self> {
        match self {
            Self::MaxCompletionTokens => None,
            Self::ContextWindow => Some(Self::MaxCompletionTokens),
            Self::Temperature => Some(Self::ContextWindow),
            Self::TopP => Some(Self::Temperature),
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

/// 多步表单的当前步骤（`Add` / `Edit` 模式使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormStep {
    /// 预设选择（第一步，仅 `Add`）。
    Preset,
    /// 渠道名称（仅自定义预设添加时出现；`Edit` 不改名）。
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
    /// 编辑已有渠道：base_url → auth_scheme → api_backend → api_key（可空保留原密钥）。
    Edit(String),
    /// `/provider set-key <name>` — 单步输入 API Key。
    SetKey(String),
    /// `/provider models <name>` — 展示渠道可用模型列表。
    Models(String),
    /// `/provider set-model <name>` — 选择并设置当前模型。
    SetModel(String),
    /// 手动输入模型 ID 并注册为当前模型（不依赖上游列表）。
    /// 确认 ID 后进入可选参数步骤（max_completion_tokens 等）。
    ManualModel(String),
    /// 选择模型后配置详细参数（写入 `[model."provider/id"]`）。
    ConfigureModel(String),
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
    // ── Add / Edit 模式的表单字段 ──
    pub name: String,
    pub base_url: String,
    pub auth_scheme_idx: usize,
    pub api_backend_idx: usize,
    pub api_key: String,
    pub current_step: FormStep,
    /// 编辑时是否已有密钥（用于 UI 提示「留空保留」）。
    pub edit_had_key: bool,
    // ── 通用状态 ──
    /// 错误消息（红色显示）。
    pub error: Option<String>,
    /// 成功消息（绿色显示，按任意键后清除）。
    pub success: Option<String>,
    /// `Models` / `SetModel` 模式下从 API 获取的模型列表。
    pub models: Vec<String>,
    /// 模型列表是否正在加载。
    pub models_loading: bool,
    /// 模型列表顶部搜索框内容（子串匹配，不区分大小写）。
    pub model_filter: String,
    /// `ManualModel` 模式下手写的模型 ID。
    pub manual_model_id: String,
    /// 模型参数表单：当前字段（`None` = 仍在 ID 输入 / 列表选择阶段）。
    pub model_param_field: Option<ModelParamField>,
    /// 单次输出 token 上限（字符串表单；空=不设）。
    pub max_completion_tokens: String,
    /// 上下文窗口（字符串表单；空=不设）。
    pub context_window: String,
    /// temperature（字符串表单；空=不设）。
    pub temperature: String,
    /// top_p（字符串表单；空=不设）。
    pub top_p: String,
    /// `List` / `Actions` / `Models` / `SetModel` 模式下的选中索引。
    /// 在模型列表中是 **过滤结果** 内的下标，不是 `models` 原始下标。
    pub selected: usize,
    /// 垂直滚动偏移（模型列表 / 渠道列表可见窗口起点）。
    pub scroll_offset: usize,
    /// 最近一次渲染时列表可见行数（供 PageUp/Down 与选中跟随滚动）。
    pub list_viewport: usize,
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
            edit_had_key: false,
            error: None,
            success: None,
            models: Vec::new(),
            models_loading: false,
            model_filter: String::new(),
            manual_model_id: String::new(),
            model_param_field: None,
            max_completion_tokens: String::new(),
            context_window: String::new(),
            temperature: String::new(),
            top_p: String::new(),
            selected: 0,
            scroll_offset: 0,
            list_viewport: 0,
            providers: Vec::new(),
        }
    }

    /// 当前过滤后的模型 id 列表（保持 API 返回顺序）。
    pub fn filtered_models(&self) -> Vec<&str> {
        let q = self.model_filter.trim();
        if q.is_empty() {
            return self.models.iter().map(|s| s.as_str()).collect();
        }
        let q_lower = q.to_lowercase();
        self.models
            .iter()
            .filter(|m| m.to_lowercase().contains(&q_lower))
            .map(|s| s.as_str())
            .collect()
    }

    /// 当前过滤列表长度。
    pub fn filtered_model_count(&self) -> usize {
        let q = self.model_filter.trim();
        if q.is_empty() {
            return self.models.len();
        }
        let q_lower = q.to_lowercase();
        self.models
            .iter()
            .filter(|m| m.to_lowercase().contains(&q_lower))
            .count()
    }

    /// 过滤结果中当前选中项的模型 id。
    pub fn selected_filtered_model(&self) -> Option<&str> {
        let filtered = self.filtered_models();
        filtered.get(self.selected).copied()
    }

    /// 记录列表可视高度，并保证当前选中行落在窗口内。
    pub fn set_list_viewport(&mut self, rows: usize, item_count: usize) {
        self.list_viewport = rows;
        self.ensure_selected_visible(item_count);
    }

    /// 将 `scroll_offset` 夹到使 `selected` 可见。
    pub fn ensure_selected_visible(&mut self, item_count: usize) {
        if item_count == 0 {
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }
        if self.selected >= item_count {
            self.selected = item_count - 1;
        }
        let viewport = self.list_viewport.max(1);
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset.saturating_add(viewport) {
            self.scroll_offset = self.selected + 1 - viewport;
        }
        let max_off = item_count.saturating_sub(viewport);
        if self.scroll_offset > max_off {
            self.scroll_offset = max_off;
        }
    }

    /// 模型列表：移动选中并滚动跟随（基于过滤结果）。
    pub fn move_models_selection(&mut self, delta: isize) {
        let len = self.filtered_model_count();
        if len == 0 {
            return;
        }
        let next = if delta >= 0 {
            (self.selected as isize + delta).min(len as isize - 1)
        } else {
            (self.selected as isize + delta).max(0)
        };
        self.selected = next as usize;
        self.ensure_selected_visible(len);
    }

    /// 模型列表：按页翻动。
    pub fn page_models(&mut self, forward: bool) {
        let len = self.filtered_model_count();
        if len == 0 {
            return;
        }
        let step = self.list_viewport.max(1) as isize;
        let delta = if forward { step } else { -step };
        self.move_models_selection(delta);
    }

    /// 向搜索框追加字符，并重置选中到过滤结果首项。
    pub fn push_model_filter_char(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        self.model_filter.push(c);
        self.on_model_filter_changed();
    }

    /// 搜索框退格。
    pub fn pop_model_filter_char(&mut self) -> bool {
        if self.model_filter.pop().is_some() {
            self.on_model_filter_changed();
            true
        } else {
            false
        }
    }

    /// 清空搜索框。
    pub fn clear_model_filter(&mut self) {
        if self.model_filter.is_empty() {
            return;
        }
        self.model_filter.clear();
        self.on_model_filter_changed();
    }

    /// 设置/粘贴搜索内容（已清洗的单行文本）。
    pub fn set_model_filter(&mut self, text: String) {
        if self.model_filter == text {
            return;
        }
        self.model_filter = text;
        self.on_model_filter_changed();
    }

    fn on_model_filter_changed(&mut self) {
        self.selected = 0;
        self.scroll_offset = 0;
        let len = self.filtered_model_count();
        self.ensure_selected_visible(len);
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
    ///
    /// 成功后把整表写入 config 的 `[model."provider/id"]`，这样 `/model` 在
    /// 仅「查看可用模型」后也能列出渠道模型。若配置尚无默认模型，则把第一项
    /// 设为 default（用户在「切换模型」里 Enter 仍会覆盖为选中项）。
    pub fn load_models_for(&mut self, name: &str) {
        self.models_loading = true;
        self.models.clear();
        self.model_filter.clear();
        self.error = None;
        self.selected = 0;
        self.scroll_offset = 0;
        match crate::slash::commands::provider::fetch_provider_models(name) {
            Ok(models) => {
                // Register into catalog while we have the list. Failures are
                // non-fatal for the modal (user can still browse the in-memory list).
                let need_default = crate::slash::commands::provider::load_config()
                    .map(|doc| {
                        crate::slash::commands::provider::configured_default_model(&doc).is_none()
                    })
                    .unwrap_or(false);
                let first = models.first().cloned();
                if let Err(e) = crate::slash::commands::provider::register_provider_models(
                    name,
                    &models,
                    if need_default {
                        first.as_deref()
                    } else {
                        None
                    },
                ) {
                    tracing::warn!(
                        provider = %name,
                        error = %e,
                        "failed to register provider models into catalog"
                    );
                }
                self.models = models;
                self.models_loading = false;
            }
            Err(e) => {
                self.error = Some(e);
                self.models_loading = false;
            }
        }
    }

    /// 清空模型参数表单字段。
    pub fn clear_model_params(&mut self) {
        self.model_param_field = None;
        self.max_completion_tokens.clear();
        self.context_window.clear();
        self.temperature.clear();
        self.top_p.clear();
    }

    /// 当前参数字段对应的可编辑字符串。
    pub fn model_param_value_mut(&mut self, field: ModelParamField) -> &mut String {
        match field {
            ModelParamField::MaxCompletionTokens => &mut self.max_completion_tokens,
            ModelParamField::ContextWindow => &mut self.context_window,
            ModelParamField::Temperature => &mut self.temperature,
            ModelParamField::TopP => &mut self.top_p,
        }
    }

    pub fn model_param_value(&self, field: ModelParamField) -> &str {
        match field {
            ModelParamField::MaxCompletionTokens => self.max_completion_tokens.as_str(),
            ModelParamField::ContextWindow => self.context_window.as_str(),
            ModelParamField::Temperature => self.temperature.as_str(),
            ModelParamField::TopP => self.top_p.as_str(),
        }
    }

    /// 从 config 预填某模型已有参数（配置模型参数用）。
    pub fn prefill_model_params(&mut self, provider: &str, model_id: &str) {
        self.clear_model_params();
        let catalog_key =
            crate::slash::commands::provider::provider_model_catalog_key(provider, model_id);
        let Ok(doc) = crate::slash::commands::provider::load_config() else {
            tracing::warn!("prefill_model_params: failed to load config");
            return;
        };
        let Some(table) = doc
            .get("model")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get(catalog_key.as_str()))
            .and_then(|v| v.as_table())
        else {
            return;
        };
        if let Some(v) = table.get("max_completion_tokens").and_then(|i| {
            i.as_integer()
                .map(|n| n.to_string())
                .or_else(|| i.as_str().map(|s| s.to_string()))
        }) {
            self.max_completion_tokens = v;
        }
        if let Some(v) = table.get("context_window").and_then(|i| {
            i.as_integer()
                .map(|n| n.to_string())
                .or_else(|| i.as_str().map(|s| s.to_string()))
        }) {
            self.context_window = v;
        }
        if let Some(v) = table.get("temperature").and_then(|i| {
            i.as_float()
                .map(|n| format_float_trim(n))
                .or_else(|| i.as_integer().map(|n| n.to_string()))
                .or_else(|| i.as_str().map(|s| s.to_string()))
        }) {
            self.temperature = v;
        }
        if let Some(v) = table.get("top_p").and_then(|i| {
            i.as_float()
                .map(|n| format_float_trim(n))
                .or_else(|| i.as_integer().map(|n| n.to_string()))
                .or_else(|| i.as_str().map(|s| s.to_string()))
        }) {
            self.top_p = v;
        }
    }

    /// 导航回渠道列表 hub。
    pub fn go_list(&mut self) {
        self.from_hub = true;
        self.mode = ProviderModalMode::List;
        self.clear_messages();
        self.api_key.clear();
        self.models.clear();
        self.model_filter.clear();
        self.manual_model_id.clear();
        self.clear_model_params();
        self.edit_had_key = false;
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
        self.model_filter.clear();
        self.manual_model_id.clear();
        self.clear_model_params();
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
        self.edit_had_key = false;
        self.current_step = FormStep::Preset;
        self.selected = 0;
    }

    /// 从 config.toml 预填编辑表单字段（base_url / auth_scheme / api_backend /
    /// edit_had_key）。公共逻辑，供 `go_edit` 和深链 `dispatch_open_provider_modal`
    /// 复用，避免两份拷贝不同步。
    pub fn prefill_edit_fields(&mut self, name: &str) -> Result<(), String> {
        let doc = crate::slash::commands::provider::load_config()?;
        self.name = name.to_string();
        self.base_url = crate::slash::commands::provider::provider_field(&doc, name, "base_url")
            .unwrap_or_default();
        let auth =
            crate::slash::commands::provider::provider_field(&doc, name, "auth_scheme")
                .unwrap_or_else(|| "bearer".into());
        self.auth_scheme_idx = AUTH_SCHEMES
            .iter()
            .position(|&s| s == auth)
            .unwrap_or(0);
        let backend =
            crate::slash::commands::provider::provider_field(&doc, name, "api_backend")
                .unwrap_or_else(|| "chat_completions".into());
        self.api_backend_idx = API_BACKENDS
            .iter()
            .position(|&s| s == backend)
            .unwrap_or(1);
        self.edit_had_key =
            crate::slash::commands::provider::provider_field(&doc, name, "api_key").is_some();
        self.current_step = FormStep::BaseUrl;
        Ok(())
    }

    /// 打开编辑表单，预填已有配置。
    pub fn go_edit(&mut self, name: String) {
        self.clear_messages();
        self.api_key.clear();
        self.manual_model_id.clear();
        self.selected = 0;

        match self.prefill_edit_fields(&name) {
            Ok(()) => {
                self.mode = ProviderModalMode::Edit(name);
            }
            Err(e) => {
                self.error = Some(e);
                self.mode = ProviderModalMode::Actions(name);
            }
        }
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

    /// 打开手动输入模型 ID。
    pub fn go_manual_model(&mut self, name: String) {
        self.mode = ProviderModalMode::ManualModel(name);
        self.clear_messages();
        self.manual_model_id.clear();
        self.clear_model_params();
        self.selected = 0;
    }

    /// 打开「配置模型参数」：先选/搜模型，再填参数。
    pub fn go_configure_model(&mut self, name: String) {
        self.mode = ProviderModalMode::ConfigureModel(name.clone());
        self.clear_messages();
        self.clear_model_params();
        self.manual_model_id.clear();
        self.load_models_for(&name);
    }

    /// Esc 逐级返回；返回 true 表示应关闭模态框。
    ///
    /// 多步表单内始终先退一步（不依赖 `from_hub`）。仅在该层级的「根」
    /// 再根据 `from_hub` 决定：回列表/操作菜单，或关闭整个模态。
    pub fn navigate_back(&mut self) -> bool {
        if self.success.is_some() {
            self.success = None;
        }
        match self.mode.clone() {
            ProviderModalMode::List => true,
            ProviderModalMode::Actions(name) => {
                if !self.from_hub {
                    return true;
                }
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
                if self.current_step != FormStep::Preset {
                    // 预设快捷路径：选预设后跳过 Name/… 直达 ApiKey 时，
                    // 从 ApiKey 一键回到预设列表。
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
                if self.from_hub {
                    self.go_list();
                    false
                } else {
                    true
                }
            }
            ProviderModalMode::Edit(name) => {
                // 编辑步骤内逐级回退；首步再决定回操作菜单或关闭。
                match self.current_step {
                    FormStep::AuthScheme => {
                        self.current_step = FormStep::BaseUrl;
                        self.error = None;
                        return false;
                    }
                    FormStep::ApiBackend => {
                        self.current_step = FormStep::AuthScheme;
                        self.error = None;
                        return false;
                    }
                    FormStep::ApiKey => {
                        self.current_step = FormStep::ApiBackend;
                        self.error = None;
                        return false;
                    }
                    _ => {}
                }
                if self.from_hub {
                    self.go_actions(name);
                    false
                } else {
                    true
                }
            }
            ProviderModalMode::ManualModel(name) => {
                // 参数步骤内回退字段；回到 ID 输入；再退出模式。
                if let Some(field) = self.model_param_field {
                    if let Some(prev) = field.prev() {
                        self.model_param_field = Some(prev);
                    } else {
                        self.model_param_field = None;
                    }
                    self.error = None;
                    return false;
                }
                if self.from_hub {
                    self.go_actions(name);
                    false
                } else {
                    true
                }
            }
            ProviderModalMode::ConfigureModel(name) => {
                if let Some(field) = self.model_param_field {
                    if let Some(prev) = field.prev() {
                        self.model_param_field = Some(prev);
                    } else {
                        // 离开参数表单，回到模型选择（保留已选 model id）
                        self.model_param_field = None;
                    }
                    self.error = None;
                    return false;
                }
                if self.from_hub {
                    self.go_actions(name);
                    false
                } else {
                    true
                }
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

fn format_float_trim(n: f64) -> String {
    let s = format!("{n}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}
