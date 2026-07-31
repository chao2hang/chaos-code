use crate::views::modal_window::ModalWindowState;
use xai_grok_shell::agent::client_profiles::{ClientProfile, by_id as builtin_profile_by_id};

pub const MODAL_TITLE: &str = "客户端选择";
pub const PROTOCOLS: &[&str] = &["responses", "chat_completions", "messages"];
pub const AUTH_SCHEMES: &[&str] = &["bearer", "x_api_key", "none"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientModalMode {
    List,
    Form { editing_id: Option<String> },
    ConfirmDelete(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientFormField {
    Id,
    Name,
    Protocol,
    AuthScheme,
    EnvKey,
    ClientIdentifier,
}

impl ClientFormField {
    pub const ALL: &[Self] = &[
        Self::Id,
        Self::Name,
        Self::Protocol,
        Self::AuthScheme,
        Self::EnvKey,
        Self::ClientIdentifier,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Id => "ID",
            Self::Name => "名称",
            Self::Protocol => "协议",
            Self::AuthScheme => "认证",
            Self::EnvKey => "环境变量",
            Self::ClientIdentifier => "请求标识",
        }
    }

    pub fn next(self, editing: bool) -> Option<Self> {
        let fields = if editing { &Self::ALL[1..] } else { Self::ALL };
        let index = fields.iter().position(|field| *field == self)?;
        fields.get(index + 1).copied()
    }

    pub fn previous(self, editing: bool) -> Option<Self> {
        let fields = if editing { &Self::ALL[1..] } else { Self::ALL };
        let index = fields.iter().position(|field| *field == self)?;
        index
            .checked_sub(1)
            .and_then(|index| fields.get(index).copied())
    }
}

#[derive(Debug)]
pub enum ClientKeyOutcome {
    Close,
    Changed,
    Unchanged,
    Select(ClientProfile),
    Commit {
        profile: ClientProfile,
        editing_id: Option<String>,
    },
    SetDefault(String),
    Delete(String),
}

pub struct ClientModalState {
    pub window: ModalWindowState,
    pub mode: ClientModalMode,
    pub profiles: Vec<ClientProfile>,
    pub default_id: Option<String>,
    /// Profile already selected for this live conversation, if known.
    pub current_id: Option<String>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub list_viewport: usize,
    pub form_field: ClientFormField,
    pub id: String,
    pub name: String,
    pub protocol_idx: usize,
    pub auth_scheme_idx: usize,
    pub env_key: String,
    pub client_identifier: String,
    pub error: Option<String>,
    pub success: Option<String>,
}

impl ClientModalState {
    pub fn new(current_id: Option<String>) -> Self {
        let mut state = Self {
            window: ModalWindowState::new(),
            mode: ClientModalMode::List,
            profiles: Vec::new(),
            default_id: None,
            current_id,
            selected: 0,
            scroll_offset: 0,
            list_viewport: 0,
            form_field: ClientFormField::Id,
            id: String::new(),
            name: String::new(),
            protocol_idx: 0,
            auth_scheme_idx: 0,
            env_key: String::new(),
            client_identifier: String::new(),
            error: None,
            success: None,
        };
        state.reload_profiles();
        state
    }

    pub fn reload_profiles(&mut self) {
        self.error = None;
        match crate::slash::commands::provider::load_config() {
            Ok(doc) => {
                self.profiles = crate::slash::commands::client::list_client_profiles(&doc);
                self.default_id = crate::slash::commands::client::configured_default_client(&doc);
                self.ensure_selected_visible();
            }
            Err(error) => {
                self.profiles.clear();
                self.error = Some(error);
            }
        }
    }

    pub fn is_builtin(profile: &ClientProfile) -> bool {
        builtin_profile_by_id(&profile.id).is_some()
    }

    pub fn selected_profile(&self) -> Option<&ClientProfile> {
        self.profiles.get(self.selected)
    }

    pub fn selected_is_custom(&self) -> bool {
        self.selected_profile()
            .is_some_and(|profile| !Self::is_builtin(profile))
    }

    pub fn ensure_selected_visible(&mut self) {
        if self.profiles.is_empty() {
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }
        if self.selected >= self.profiles.len() {
            self.selected = self.profiles.len() - 1;
        }
        let viewport = self.list_viewport.max(1);
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + viewport {
            self.scroll_offset = self.selected + 1 - viewport;
        }
        let max_offset = self.profiles.len().saturating_sub(viewport);
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.profiles.is_empty() {
            return;
        }
        let next =
            (self.selected as isize + delta).clamp(0, self.profiles.len() as isize - 1) as usize;
        self.selected = next;
        self.ensure_selected_visible();
    }

    pub fn select_id(&mut self, id: &str) {
        if let Some(index) = self.profiles.iter().position(|profile| profile.id == id) {
            self.selected = index;
            self.ensure_selected_visible();
        }
    }

    pub fn start_add(&mut self) {
        self.mode = ClientModalMode::Form { editing_id: None };
        self.form_field = ClientFormField::Id;
        self.id.clear();
        self.name.clear();
        self.protocol_idx = 0;
        self.auth_scheme_idx = 0;
        self.env_key.clear();
        self.client_identifier.clear();
        self.error = None;
        self.success = None;
    }

    pub fn start_edit(&mut self) -> bool {
        let Some(profile) = self.selected_profile().cloned() else {
            return false;
        };
        if Self::is_builtin(&profile) {
            self.error = Some("内置客户端不能编辑；可以新增自定义客户端".into());
            return false;
        }
        self.mode = ClientModalMode::Form {
            editing_id: Some(profile.id.clone()),
        };
        self.form_field = ClientFormField::Name;
        self.id = profile.id;
        self.name = profile.name;
        self.protocol_idx = PROTOCOLS
            .iter()
            .position(|protocol| *protocol == profile.protocol)
            .unwrap_or(0);
        self.auth_scheme_idx = AUTH_SCHEMES
            .iter()
            .position(|scheme| *scheme == profile.auth_scheme)
            .unwrap_or(0);
        self.env_key = profile.env_key;
        self.client_identifier = profile.client_identifier;
        self.error = None;
        self.success = None;
        true
    }

    pub fn editing_id(&self) -> Option<&str> {
        match &self.mode {
            ClientModalMode::Form { editing_id } => editing_id.as_deref(),
            _ => None,
        }
    }

    pub fn current_protocol(&self) -> &'static str {
        PROTOCOLS[self.protocol_idx.min(PROTOCOLS.len() - 1)]
    }

    pub fn current_auth_scheme(&self) -> &'static str {
        AUTH_SCHEMES[self.auth_scheme_idx.min(AUTH_SCHEMES.len() - 1)]
    }

    pub fn build_profile(&self) -> ClientProfile {
        ClientProfile {
            id: self.id.clone(),
            name: self.name.clone(),
            protocol: self.current_protocol().to_owned(),
            auth_scheme: self.current_auth_scheme().to_owned(),
            env_key: self.env_key.clone(),
            client_identifier: self.client_identifier.clone(),
        }
    }

    pub fn push_text(&mut self, value: char) {
        if value.is_control() {
            return;
        }
        match self.form_field {
            ClientFormField::Id => self.id.push(value),
            ClientFormField::Name => self.name.push(value),
            ClientFormField::EnvKey => self.env_key.push(value),
            ClientFormField::ClientIdentifier => self.client_identifier.push(value),
            ClientFormField::Protocol | ClientFormField::AuthScheme => {}
        }
        self.error = None;
    }

    pub fn pop_text(&mut self) -> bool {
        let field = match self.form_field {
            ClientFormField::Id => &mut self.id,
            ClientFormField::Name => &mut self.name,
            ClientFormField::EnvKey => &mut self.env_key,
            ClientFormField::ClientIdentifier => &mut self.client_identifier,
            ClientFormField::Protocol | ClientFormField::AuthScheme => return false,
        };
        let changed = field.pop().is_some();
        if changed {
            self.error = None;
        }
        changed
    }

    pub fn set_pasted_text(&mut self, value: String) -> bool {
        let field = match self.form_field {
            ClientFormField::Id => &mut self.id,
            ClientFormField::Name => &mut self.name,
            ClientFormField::EnvKey => &mut self.env_key,
            ClientFormField::ClientIdentifier => &mut self.client_identifier,
            ClientFormField::Protocol | ClientFormField::AuthScheme => return false,
        };
        if field.is_empty() {
            *field = value;
        } else {
            field.push_str(&value);
        }
        self.error = None;
        true
    }
}
