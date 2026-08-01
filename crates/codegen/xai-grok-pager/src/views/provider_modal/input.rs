//! Provider modal keyboard input handling.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::state::{
    API_BACKENDS, AUTH_SCHEMES, FormStep, ModelParamField, PROVIDER_PRESETS, ProviderAction,
    ProviderKeyOutcome, ProviderModalMode, ProviderModalState,
};

/// Sanitize text for a single-line provider field.
///
/// Windows clipboard / terminal paste often includes trailing `\r\n`. If left
/// in `api_key`, TOML stores the CR and HTTP auth headers / re-reads look like
/// "a few characters vanished". Strip line breaks and outer whitespace; for
/// multi-line paste (accidental full config dump) keep the first non-empty line.
pub(crate) fn sanitize_provider_field(text: &str) -> String {
    // Prefer first non-empty line so a trailing Enter from clipboard doesn't
    // become a bare newline-only field, and multi-line dumps don't embed `\n`.
    let line = text
        .lines()
        .map(|l| l.trim_matches(|c| c == '\r' || c == '\n' || c == '\u{feff}'))
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .trim();
    // Defensive: lines() already drops `\n`, but bare `\r` mid-string can remain.
    line.chars().filter(|c| !matches!(c, '\r' | '\n')).collect()
}

/// 处理模态框按键事件。
pub fn handle_provider_key(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    if key.kind == KeyEventKind::Release {
        return ProviderKeyOutcome::Unchanged;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('g'))
    {
        return ProviderKeyOutcome::Close;
    }

    // Ctrl/Cmd/Alt+V: read the system clipboard (same as the main prompt).
    // Relying only on `Event::Paste` fails when Windows Terminal swallows
    // Ctrl+V as a terminal action or when bracketed paste is incomplete.
    if crate::input::key::is_paste_key(key) {
        match crate::clipboard::system_clipboard_read_text() {
            Ok(Some(text)) => return handle_provider_paste(state, &text),
            Ok(None) => return ProviderKeyOutcome::Unchanged,
            Err(_) => return ProviderKeyOutcome::Unchanged,
        }
    }

    if state.success.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                let msg = state.success.take();
                // Hub：成功后回到列表；深链：关闭
                if state.from_hub {
                    // 添加成功后回到列表；编辑 / 设 key 后回到操作菜单
                    if matches!(state.mode, ProviderModalMode::Add) {
                        state.go_list();
                    } else if let ProviderModalMode::SetKey(name)
                    | ProviderModalMode::Edit(name)
                    | ProviderModalMode::ManualModel(name)
                    | ProviderModalMode::ConfigureModel(name) = &state.mode
                    {
                        let n = name.clone();
                        state.go_actions(n);
                        if let Some(m) = msg {
                            state.success = Some(m);
                        }
                    } else {
                        state.go_list();
                    }
                    return ProviderKeyOutcome::Changed;
                }
                return ProviderKeyOutcome::Close;
            }
            _ => {
                state.success = None;
                return ProviderKeyOutcome::Changed;
            }
        }
    }

    match &state.mode {
        ProviderModalMode::List => handle_list(state, key),
        ProviderModalMode::Actions(_) => handle_actions(state, key),
        ProviderModalMode::Add => handle_add(state, key),
        ProviderModalMode::Edit(_) => handle_edit(state, key),
        ProviderModalMode::SetKey(_) => handle_set_key(state, key),
        ProviderModalMode::Models(_) => handle_models(state, key),
        ProviderModalMode::SetModel(_) => handle_set_model(state, key),
        ProviderModalMode::ManualModel(_) => handle_manual_model(state, key),
        ProviderModalMode::ConfigureModel(_) => handle_configure_model(state, key),
        ProviderModalMode::ConfirmingDelete(_) => handle_confirm_delete(state, key),
    }
}

/// 处理粘贴事件——将文本粘贴到当前输入字段。
pub fn handle_provider_paste(state: &mut ProviderModalState, text: &str) -> ProviderKeyOutcome {
    if state.success.is_some() {
        return ProviderKeyOutcome::Unchanged;
    }
    let cleaned = sanitize_provider_field(text);
    if cleaned.is_empty() {
        return ProviderKeyOutcome::Unchanged;
    }
    match &state.mode {
        ProviderModalMode::Add | ProviderModalMode::Edit(_) => {
            let field = match state.current_step {
                FormStep::Name => &mut state.name,
                FormStep::BaseUrl => &mut state.base_url,
                FormStep::ApiKey => &mut state.api_key,
                _ => return ProviderKeyOutcome::Unchanged,
            };
            // Replace when the field is empty so a full key paste is clean;
            // append when the user is extending an existing draft.
            if field.is_empty() {
                *field = cleaned;
            } else {
                field.push_str(&cleaned);
            }
            state.error = None;
            ProviderKeyOutcome::Changed
        }
        ProviderModalMode::SetKey(_) => {
            if state.api_key.is_empty() {
                state.api_key = cleaned;
            } else {
                state.api_key.push_str(&cleaned);
            }
            state.error = None;
            ProviderKeyOutcome::Changed
        }
        ProviderModalMode::ManualModel(_) => {
            if let Some(field) = state.model_param_field {
                let slot = state.model_param_value_mut(field);
                if slot.is_empty() {
                    *slot = cleaned;
                } else {
                    slot.push_str(&cleaned);
                }
            } else if state.manual_model_id.is_empty() {
                state.manual_model_id = cleaned;
            } else {
                state.manual_model_id.push_str(&cleaned);
            }
            state.error = None;
            ProviderKeyOutcome::Changed
        }
        ProviderModalMode::ConfigureModel(_) => {
            if let Some(field) = state.model_param_field {
                let slot = state.model_param_value_mut(field);
                if slot.is_empty() {
                    *slot = cleaned;
                } else {
                    slot.push_str(&cleaned);
                }
                state.error = None;
                ProviderKeyOutcome::Changed
            } else {
                // 选择阶段：粘贴进搜索框
                if state.model_filter.is_empty() {
                    state.set_model_filter(cleaned);
                } else {
                    let mut next = state.model_filter.clone();
                    next.push_str(&cleaned);
                    state.set_model_filter(next);
                }
                ProviderKeyOutcome::Changed
            }
        }
        ProviderModalMode::Models(_) | ProviderModalMode::SetModel(_) => {
            // Paste into the model search bar (append when refining query).
            if state.model_filter.is_empty() {
                state.set_model_filter(cleaned);
            } else {
                let mut next = state.model_filter.clone();
                next.push_str(&cleaned);
                state.set_model_filter(next);
            }
            ProviderKeyOutcome::Changed
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

fn handle_add(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    match key.code {
        KeyCode::Esc => {
            if state.navigate_back() {
                ProviderKeyOutcome::Close
            } else {
                ProviderKeyOutcome::Changed
            }
        }
        // Arrow keys only — do NOT bind bare j/k here. On Name/BaseUrl/ApiKey
        // those letters must type into the field (API keys often contain j/k).
        // List-style j/k nav is limited to Preset (and AuthScheme/ApiBackend
        // via handle_select_key on Char).
        KeyCode::Up => {
            if state.current_step == FormStep::Preset {
                if state.selected > 0 {
                    state.selected -= 1;
                }
                ProviderKeyOutcome::Changed
            } else if state.current_step == FormStep::ClinePick {
                if state.selected > 0 {
                    state.selected -= 1;
                }
                ProviderKeyOutcome::Changed
            } else if matches!(
                state.current_step,
                FormStep::AuthScheme | FormStep::ApiBackend
            ) {
                handle_select_key(state, key)
            } else {
                ProviderKeyOutcome::Unchanged
            }
        }
        KeyCode::Down => {
            if state.current_step == FormStep::Preset {
                let max = state.preset_row_count() - 1;
                if state.selected < max {
                    state.selected += 1;
                }
                ProviderKeyOutcome::Changed
            } else if state.current_step == FormStep::ClinePick {
                let max = state.cline_candidates.len().saturating_sub(1);
                if state.selected < max {
                    state.selected += 1;
                }
                ProviderKeyOutcome::Changed
            } else if matches!(
                state.current_step,
                FormStep::AuthScheme | FormStep::ApiBackend
            ) {
                handle_select_key(state, key)
            } else {
                ProviderKeyOutcome::Unchanged
            }
        }
        // j/k only navigate on the preset list, never on free-text steps.
        KeyCode::Char('j')
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && (state.current_step == FormStep::Preset
                    || state.current_step == FormStep::ClinePick) =>
        {
            let max = if state.current_step == FormStep::Preset {
                state.preset_row_count() - 1
            } else {
                state.cline_candidates.len().saturating_sub(1)
            };
            if state.selected < max {
                state.selected += 1;
            }
            ProviderKeyOutcome::Changed
        }
        KeyCode::Char('k')
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && (state.current_step == FormStep::Preset
                    || state.current_step == FormStep::ClinePick) =>
        {
            if state.selected > 0 {
                state.selected -= 1;
            }
            ProviderKeyOutcome::Changed
        }
        KeyCode::Tab | KeyCode::Enter => match state.current_step {
            FormStep::Preset => {
                if state.selected < PROVIDER_PRESETS.len() {
                    let p = &PROVIDER_PRESETS[state.selected];
                    state.name = p.name.to_string();
                    state.base_url = p.base_url.to_string();
                    state.auth_scheme_idx = AUTH_SCHEMES
                        .iter()
                        .position(|&s| s == p.auth_scheme)
                        .unwrap_or(0);
                    state.api_backend_idx = API_BACKENDS
                        .iter()
                        .position(|&s| s == p.api_backend)
                        .unwrap_or(1);
                    state.current_step = FormStep::ApiKey;
                } else if state.selected == PROVIDER_PRESETS.len() {
                    // 自定义
                    state.current_step = FormStep::Name;
                    state.name.clear();
                    state.base_url.clear();
                } else if state.has_cline_option() && state.selected == state.cline_option_idx() {
                    // 从 Cline 导入
                    state.current_step = FormStep::ClinePick;
                    state.selected = 0;
                } else {
                    return ProviderKeyOutcome::Unchanged;
                }
                state.error = None;
                ProviderKeyOutcome::Changed
            }
            FormStep::ClinePick => {
                let Some(candidate) = state.cline_candidates.get(state.selected).cloned() else {
                    return ProviderKeyOutcome::Unchanged;
                };
                // Fill form fields from the Cline candidate.
                state.name = candidate.id;
                state.base_url = candidate.base_url;
                state.auth_scheme_idx = AUTH_SCHEMES
                    .iter()
                    .position(|&s| s == candidate.auth_scheme)
                    .unwrap_or(0);
                state.api_backend_idx = API_BACKENDS
                    .iter()
                    .position(|&s| s == candidate.api_backend)
                    .unwrap_or(1);
                state.api_key = candidate.api_key.unwrap_or_default();
                state.error = None;
                if candidate.key_encrypted {
                    // Key is safeStorage ciphertext — guide user to paste manually.
                    state.current_step = FormStep::ApiKey;
                    state.api_key.clear();
                    state.error = Some(
                        "该 Key 已被 Cline 加密（safeStorage），请在下方手动粘贴 API Key".into(),
                    );
                    ProviderKeyOutcome::Changed
                } else if state.api_key.is_empty() {
                    // No key at all — go to ApiKey step.
                    state.current_step = FormStep::ApiKey;
                    ProviderKeyOutcome::Changed
                } else {
                    // Everything filled — ready to commit.
                    state.current_step = FormStep::ApiKey;
                    ProviderKeyOutcome::Changed
                }
            }
            FormStep::Name => {
                if state.name.is_empty() {
                    state.error = Some("不能为空".into());
                    return ProviderKeyOutcome::Changed;
                }
                state.error = None;
                state.current_step = FormStep::BaseUrl;
                ProviderKeyOutcome::Changed
            }
            FormStep::BaseUrl => {
                if state.base_url.is_empty() {
                    state.error = Some("不能为空".into());
                    return ProviderKeyOutcome::Changed;
                }
                state.error = None;
                state.current_step = FormStep::AuthScheme;
                ProviderKeyOutcome::Changed
            }
            FormStep::AuthScheme => {
                state.current_step = FormStep::ApiBackend;
                ProviderKeyOutcome::Changed
            }
            FormStep::ApiBackend => {
                state.current_step = FormStep::ApiKey;
                ProviderKeyOutcome::Changed
            }
            FormStep::ApiKey => {
                // Normalize again at commit (typed keys can still carry paste
                // artifacts if paste arrived as keystrokes with trailing CR).
                state.api_key = sanitize_provider_field(&state.api_key);
                if state.api_key.is_empty() {
                    state.error = Some("不能为空".into());
                    return ProviderKeyOutcome::Changed;
                }
                state.error = None;
                ProviderKeyOutcome::Commit
            }
        },
        KeyCode::Backspace => {
            let field = match state.current_step {
                FormStep::Name => &mut state.name,
                FormStep::BaseUrl => &mut state.base_url,
                FormStep::ApiKey => &mut state.api_key,
                _ => return ProviderKeyOutcome::Unchanged,
            };
            if field.pop().is_some() {
                state.error = None;
                ProviderKeyOutcome::Changed
            } else {
                ProviderKeyOutcome::Unchanged
            }
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            match state.current_step {
                FormStep::Preset | FormStep::ClinePick => ProviderKeyOutcome::Unchanged,
                FormStep::Name => {
                    state.name.push(c);
                    state.error = None;
                    ProviderKeyOutcome::Changed
                }
                FormStep::BaseUrl => {
                    state.base_url.push(c);
                    state.error = None;
                    ProviderKeyOutcome::Changed
                }
                FormStep::ApiKey => {
                    state.api_key.push(c);
                    state.error = None;
                    ProviderKeyOutcome::Changed
                }
                FormStep::AuthScheme | FormStep::ApiBackend => handle_select_key(state, key),
            }
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

fn handle_select_key(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    let (choice_count, idx) = match state.current_step {
        FormStep::AuthScheme => (AUTH_SCHEMES.len(), &mut state.auth_scheme_idx),
        FormStep::ApiBackend => (API_BACKENDS.len(), &mut state.api_backend_idx),
        _ => return ProviderKeyOutcome::Unchanged,
    };
    match key.code {
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Right | KeyCode::Char('l') => {
            if choice_count == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            *idx = (*idx + 1) % choice_count;
            ProviderKeyOutcome::Changed
        }
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Left | KeyCode::Char('h') => {
            if choice_count == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            *idx = if *idx == 0 {
                choice_count - 1
            } else {
                *idx - 1
            };
            ProviderKeyOutcome::Changed
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

fn handle_set_key(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    match key.code {
        KeyCode::Esc => {
            if state.navigate_back() {
                ProviderKeyOutcome::Close
            } else {
                ProviderKeyOutcome::Changed
            }
        }
        KeyCode::Enter => {
            state.api_key = sanitize_provider_field(&state.api_key);
            if state.api_key.is_empty() {
                state.error = Some("API Key 不能为空".into());
                return ProviderKeyOutcome::Changed;
            }
            state.error = None;
            ProviderKeyOutcome::Commit
        }
        KeyCode::Backspace => {
            if state.api_key.pop().is_some() {
                state.error = None;
                ProviderKeyOutcome::Changed
            } else {
                ProviderKeyOutcome::Unchanged
            }
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.api_key.push(c);
            state.error = None;
            ProviderKeyOutcome::Changed
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

/// 编辑已有渠道：BaseUrl → AuthScheme → ApiBackend → ApiKey（可空保留原密钥）。
fn handle_edit(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    match key.code {
        KeyCode::Esc => {
            if state.navigate_back() {
                ProviderKeyOutcome::Close
            } else {
                ProviderKeyOutcome::Changed
            }
        }
        KeyCode::Up | KeyCode::Down => {
            if matches!(
                state.current_step,
                FormStep::AuthScheme | FormStep::ApiBackend
            ) {
                handle_select_key(state, key)
            } else {
                ProviderKeyOutcome::Unchanged
            }
        }
        KeyCode::Tab | KeyCode::Enter => match state.current_step {
            FormStep::BaseUrl => {
                if state.base_url.trim().is_empty() {
                    state.error = Some("不能为空".into());
                    return ProviderKeyOutcome::Changed;
                }
                state.error = None;
                state.current_step = FormStep::AuthScheme;
                ProviderKeyOutcome::Changed
            }
            FormStep::AuthScheme => {
                state.current_step = FormStep::ApiBackend;
                ProviderKeyOutcome::Changed
            }
            FormStep::ApiBackend => {
                state.current_step = FormStep::ApiKey;
                ProviderKeyOutcome::Changed
            }
            FormStep::ApiKey => {
                state.api_key = sanitize_provider_field(&state.api_key);
                // 允许留空：保留原密钥。若原本也没有密钥则提示。
                if state.api_key.is_empty() && !state.edit_had_key {
                    state.error = Some("尚未设置 API Key，请输入".into());
                    return ProviderKeyOutcome::Changed;
                }
                state.error = None;
                ProviderKeyOutcome::Commit
            }
            // Edit 不走 Preset/ClinePick/Name
            FormStep::Preset | FormStep::ClinePick | FormStep::Name => {
                ProviderKeyOutcome::Unchanged
            }
        },
        KeyCode::Backspace => {
            let field = match state.current_step {
                FormStep::BaseUrl => &mut state.base_url,
                FormStep::ApiKey => &mut state.api_key,
                _ => return ProviderKeyOutcome::Unchanged,
            };
            if field.pop().is_some() {
                state.error = None;
                ProviderKeyOutcome::Changed
            } else {
                ProviderKeyOutcome::Unchanged
            }
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            match state.current_step {
                FormStep::BaseUrl => {
                    state.base_url.push(c);
                    state.error = None;
                    ProviderKeyOutcome::Changed
                }
                FormStep::ApiKey => {
                    state.api_key.push(c);
                    state.error = None;
                    ProviderKeyOutcome::Changed
                }
                FormStep::AuthScheme | FormStep::ApiBackend => handle_select_key(state, key),
                _ => ProviderKeyOutcome::Unchanged,
            }
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

/// 手动输入模型 ID，再可选填 max_completion_tokens 等；最后 SwitchModel。
fn handle_manual_model(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    // 参数字段步骤：与 ConfigureModel 共用编辑逻辑。
    if state.model_param_field.is_some() {
        return handle_model_param_fields(state, key, /* finalize */ true);
    }
    match key.code {
        KeyCode::Esc => {
            if state.navigate_back() {
                ProviderKeyOutcome::Close
            } else {
                ProviderKeyOutcome::Changed
            }
        }
        // Tab / Enter：校验 ID → 进入参数表单第一步。
        KeyCode::Tab | KeyCode::Enter => {
            let id = sanitize_provider_field(&state.manual_model_id);
            state.manual_model_id = id.clone();
            if id.is_empty() {
                state.error = Some("模型 ID 不能为空".into());
                return ProviderKeyOutcome::Changed;
            }
            state.error = None;
            state.model_param_field = Some(ModelParamField::MaxCompletionTokens);
            ProviderKeyOutcome::Changed
        }
        KeyCode::Backspace => {
            if state.manual_model_id.pop().is_some() {
                state.error = None;
                ProviderKeyOutcome::Changed
            } else {
                ProviderKeyOutcome::Unchanged
            }
        }
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !key.modifiers.contains(KeyModifiers::SUPER) =>
        {
            state.manual_model_id.push(c);
            state.error = None;
            ProviderKeyOutcome::Changed
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

/// 配置模型参数：先选模型（列表），再填参数，最后 Commit。
fn handle_configure_model(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    if state.model_param_field.is_some() {
        return handle_model_param_fields(state, key, /* finalize */ false);
    }
    // 列表选择阶段。Enter → 进入参数表单。
    match key.code {
        KeyCode::Esc => {
            if !state.model_filter.is_empty() {
                state.clear_model_filter();
                return ProviderKeyOutcome::Changed;
            }
            if state.navigate_back() {
                ProviderKeyOutcome::Close
            } else {
                ProviderKeyOutcome::Changed
            }
        }
        KeyCode::Enter => {
            let Some(id) = state.selected_filtered_model().map(|s| s.to_string()) else {
                // 无列表时允许手写到 filter 作为 ID
                let typed = sanitize_provider_field(&state.model_filter);
                if typed.is_empty() {
                    state.error = Some("请选择或搜索模型".into());
                    return ProviderKeyOutcome::Changed;
                }
                state.manual_model_id = typed.clone();
                let provider = match &state.mode {
                    ProviderModalMode::ConfigureModel(n) => n.clone(),
                    _ => String::new(),
                };
                state.prefill_model_params(&provider, &typed);
                state.model_param_field = Some(ModelParamField::MaxCompletionTokens);
                state.error = None;
                return ProviderKeyOutcome::Changed;
            };
            let provider = match &state.mode {
                ProviderModalMode::ConfigureModel(n) => n.clone(),
                _ => String::new(),
            };
            state.manual_model_id = id.clone();
            state.prefill_model_params(&provider, &id);
            state.model_param_field = Some(ModelParamField::MaxCompletionTokens);
            state.error = None;
            ProviderKeyOutcome::Changed
        }
        // 复用模型列表导航/搜索
        _ => handle_model_list_keys(state, key, /* allow_enter_switch */ false),
    }
}

/// 编辑 max_completion_tokens / context_window / temperature / top_p。
///
/// `finalize_as_switch`：true 时最后一步触发 SwitchModel（手动添加）；
/// false 时最后一步 Commit（仅写参数）。
fn handle_model_param_fields(
    state: &mut ProviderModalState,
    key: &KeyEvent,
    finalize_as_switch: bool,
) -> ProviderKeyOutcome {
    let Some(field) = state.model_param_field else {
        return ProviderKeyOutcome::Unchanged;
    };
    match key.code {
        KeyCode::Esc => {
            if state.navigate_back() {
                ProviderKeyOutcome::Close
            } else {
                ProviderKeyOutcome::Changed
            }
        }
        KeyCode::Enter
            if finalize_as_switch
                && field == ModelParamField::MaxCompletionTokens
                && state.model_param_value(field).is_empty() =>
        {
            // ManualModel: 第一个参数字段为空时 Enter 直接提交，跳过所有参数。
            state.model_param_field = None;
            let id = sanitize_provider_field(&state.manual_model_id);
            if id.is_empty() {
                state.error = Some("模型 ID 不能为空".into());
                return ProviderKeyOutcome::Changed;
            }
            ProviderKeyOutcome::SwitchModel(id)
        }
        KeyCode::Tab | KeyCode::Enter => {
            // 轻量校验当前字段，再前进或提交
            if let Err(e) = validate_param_field(field, state.model_param_value(field)) {
                state.error = Some(e);
                return ProviderKeyOutcome::Changed;
            }
            state.error = None;
            if let Some(next) = field.next() {
                state.model_param_field = Some(next);
                return ProviderKeyOutcome::Changed;
            }
            // 最后一步
            if finalize_as_switch {
                let id = sanitize_provider_field(&state.manual_model_id);
                if id.is_empty() {
                    state.error = Some("模型 ID 不能为空".into());
                    return ProviderKeyOutcome::Changed;
                }
                ProviderKeyOutcome::SwitchModel(id)
            } else {
                ProviderKeyOutcome::Commit
            }
        }
        KeyCode::Backspace => {
            let slot = state.model_param_value_mut(field);
            if slot.pop().is_some() {
                state.error = None;
                ProviderKeyOutcome::Changed
            } else {
                ProviderKeyOutcome::Unchanged
            }
        }
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !key.modifiers.contains(KeyModifiers::SUPER) =>
        {
            let allow = match field {
                ModelParamField::MaxCompletionTokens | ModelParamField::ContextWindow => {
                    c.is_ascii_digit()
                }
                ModelParamField::Temperature | ModelParamField::TopP => {
                    c.is_ascii_digit() || c == '.' || c == '-'
                }
            };
            if !allow {
                return ProviderKeyOutcome::Unchanged;
            }
            state.model_param_value_mut(field).push(c);
            state.error = None;
            ProviderKeyOutcome::Changed
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

fn validate_param_field(field: ModelParamField, raw: &str) -> Result<(), String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(());
    }
    match field {
        ModelParamField::MaxCompletionTokens => {
            crate::slash::commands::provider::parse_optional_u32(s, field.label())?;
        }
        ModelParamField::ContextWindow => {
            crate::slash::commands::provider::parse_optional_u64(s, field.label())?;
        }
        ModelParamField::Temperature => {
            let v = crate::slash::commands::provider::parse_optional_f64(s, field.label())?;
            if let Some(t) = v
                && !(0.0..=2.0).contains(&t)
            {
                return Err("temperature 建议范围 0–2".into());
            }
        }
        ModelParamField::TopP => {
            let v = crate::slash::commands::provider::parse_optional_f64(s, field.label())?;
            if let Some(p) = v
                && !(0.0..=1.0).contains(&p)
            {
                return Err("top_p 必须在 0–1 之间".into());
            }
        }
    }
    Ok(())
}

fn handle_list(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    let len = state.list_row_count();
    match key.code {
        KeyCode::Esc => ProviderKeyOutcome::Close,
        KeyCode::Down | KeyCode::Char('j') => {
            if len == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            state.selected = (state.selected + 1).min(len - 1);
            ProviderKeyOutcome::Changed
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if len == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            state.selected = state.selected.saturating_sub(1);
            ProviderKeyOutcome::Changed
        }
        KeyCode::Enter => {
            if state.list_add_selected() {
                state.from_hub = true;
                state.go_add();
                return ProviderKeyOutcome::Changed;
            }
            if let Some(p) = state.providers.get(state.selected) {
                let name = p.name.clone();
                state.go_actions(name);
                return ProviderKeyOutcome::Changed;
            }
            ProviderKeyOutcome::Unchanged
        }
        KeyCode::Char('a') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.from_hub = true;
            state.go_add();
            ProviderKeyOutcome::Changed
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

fn handle_actions(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    let len = ProviderAction::ALL.len();
    match key.code {
        KeyCode::Esc => {
            if state.navigate_back() {
                ProviderKeyOutcome::Close
            } else {
                ProviderKeyOutcome::Changed
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if len == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            state.selected = (state.selected + 1).min(len - 1);
            ProviderKeyOutcome::Changed
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if len == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            state.selected = state.selected.saturating_sub(1);
            ProviderKeyOutcome::Changed
        }
        KeyCode::Enter => {
            let ProviderModalMode::Actions(name) = &state.mode else {
                return ProviderKeyOutcome::Unchanged;
            };
            let name = name.clone();
            let action = ProviderAction::ALL
                .get(state.selected)
                .copied()
                .unwrap_or(ProviderAction::Edit);
            match action {
                ProviderAction::Edit => state.go_edit(name),
                ProviderAction::SetKey => state.go_set_key(name),
                ProviderAction::Models | ProviderAction::Refresh => state.go_models(name),
                ProviderAction::ManualModel => state.go_manual_model(name),
                ProviderAction::ConfigureModel => state.go_configure_model(name),
                ProviderAction::SetModel => state.go_set_model(name),
                ProviderAction::Delete => state.go_confirm_delete(name),
            }
            ProviderKeyOutcome::Changed
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

/// 「确认删除渠道」对话框的按键处理。
///
/// - `y` / `Y` / `Enter`：执行删除
/// - `n` / `N` / `Esc`：取消（Esc 走 `navigate_back` 已回到 Actions，这里仅显式 n 走同一路径）
/// - 其它键：忽略
fn handle_confirm_delete(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            state.apply_confirm_delete();
            ProviderKeyOutcome::Changed
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            if let ProviderModalMode::ConfirmingDelete(name) = state.mode.clone() {
                state.go_actions(name);
            }
            ProviderKeyOutcome::Changed
        }
        // Esc 已由 `handle_provider_key` 顶部在 success 之外的通用回退里走
        // `navigate_back` —— 这里只对 navigate_back 的返回值翻译成 outcome。
        KeyCode::Esc => {
            if state.navigate_back() {
                ProviderKeyOutcome::Close
            } else {
                ProviderKeyOutcome::Changed
            }
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

fn handle_models(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    handle_model_list_keys(state, key, /* allow_enter_switch */ state.from_hub)
}

fn handle_set_model(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    handle_model_list_keys(state, key, /* allow_enter_switch */ true)
}

/// Shared nav for Models / SetModel: search bar + ↑↓, PgUp/PgDn, Home/End, Enter.
///
/// Typing printable characters filters the list (top search bar). Navigation
/// uses arrows / PgUp/PgDn only — bare j/k type into the filter so model-id
/// substrings with those letters still work.
fn handle_model_list_keys(
    state: &mut ProviderModalState,
    key: &KeyEvent,
    allow_enter_switch: bool,
) -> ProviderKeyOutcome {
    let len = state.filtered_model_count();
    match key.code {
        KeyCode::Esc => {
            // First Esc clears an active filter; second leaves the list.
            if !state.model_filter.is_empty() {
                state.clear_model_filter();
                return ProviderKeyOutcome::Changed;
            }
            if state.navigate_back() {
                ProviderKeyOutcome::Close
            } else {
                ProviderKeyOutcome::Changed
            }
        }
        KeyCode::Down => {
            if len == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            state.move_models_selection(1);
            ProviderKeyOutcome::Changed
        }
        KeyCode::Up => {
            if len == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            state.move_models_selection(-1);
            ProviderKeyOutcome::Changed
        }
        KeyCode::PageDown => {
            if len == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            state.page_models(true);
            ProviderKeyOutcome::Changed
        }
        KeyCode::PageUp => {
            if len == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            state.page_models(false);
            ProviderKeyOutcome::Changed
        }
        KeyCode::Home if key.modifiers.is_empty() => {
            if len == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            state.selected = 0;
            state.ensure_selected_visible(len);
            ProviderKeyOutcome::Changed
        }
        KeyCode::End if key.modifiers.is_empty() => {
            if len == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            state.selected = len - 1;
            state.ensure_selected_visible(len);
            ProviderKeyOutcome::Changed
        }
        KeyCode::Backspace => {
            if state.pop_model_filter_char() {
                ProviderKeyOutcome::Changed
            } else {
                ProviderKeyOutcome::Unchanged
            }
        }
        // Ctrl+U clears the search box (common terminal binding).
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if state.model_filter.is_empty() {
                return ProviderKeyOutcome::Unchanged;
            }
            state.clear_model_filter();
            ProviderKeyOutcome::Changed
        }
        KeyCode::Enter if allow_enter_switch => match state.selected_filtered_model() {
            Some(id) => ProviderKeyOutcome::SwitchModel(id.to_string()),
            None => ProviderKeyOutcome::Unchanged,
        },
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !key.modifiers.contains(KeyModifiers::SUPER) =>
        {
            // Printable chars refine the filter (including j/k/h/l).
            state.push_model_filter_char(c);
            ProviderKeyOutcome::Changed
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

#[cfg(test)]
mod tests {
    use super::super::state::ProviderSummary;
    use super::*;

    #[test]
    fn sanitize_strips_windows_crlf_and_bom() {
        assert_eq!(sanitize_provider_field("sk-abc123\r\n"), "sk-abc123");
        assert_eq!(sanitize_provider_field("\u{feff}sk-abc123\r"), "sk-abc123");
        assert_eq!(sanitize_provider_field("  sk-abc123  \n"), "sk-abc123");
    }

    #[test]
    fn sanitize_keeps_first_non_empty_line() {
        assert_eq!(sanitize_provider_field("sk-first\nsk-second\n"), "sk-first");
        assert_eq!(sanitize_provider_field("\r\n\r\nsk-only\r\n"), "sk-only");
    }

    #[test]
    fn paste_into_empty_api_key_replaces_not_double() {
        let mut state = ProviderModalState::new(ProviderModalMode::SetKey("openai".into()));
        assert!(matches!(
            handle_provider_paste(&mut state, "sk-paste-me\r\n"),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.api_key, "sk-paste-me");
        assert!(matches!(
            handle_provider_paste(&mut state, "-suffix\r\n"),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.api_key, "sk-paste-me-suffix");
    }

    #[test]
    fn api_key_step_accepts_j_and_k_as_text() {
        let mut state = ProviderModalState::new(ProviderModalMode::Add);
        state.current_step = FormStep::ApiKey;
        state.name = "openai".into();
        state.base_url = "https://api.openai.com/v1".into();

        for c in "sk-jack".chars() {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            assert!(
                matches!(
                    handle_provider_key(&mut state, &key),
                    ProviderKeyOutcome::Changed
                ),
                "char {c:?} must insert on ApiKey step"
            );
        }
        assert_eq!(state.api_key, "sk-jack");
    }

    #[test]
    fn name_step_accepts_j_and_k_as_text() {
        let mut state = ProviderModalState::new(ProviderModalMode::Add);
        state.current_step = FormStep::Name;
        for c in "jack".chars() {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            assert!(matches!(
                handle_provider_key(&mut state, &key),
                ProviderKeyOutcome::Changed
            ));
        }
        assert_eq!(state.name, "jack");
    }

    #[test]
    fn set_key_mode_accepts_j_and_k_as_text() {
        let mut state = ProviderModalState::new(ProviderModalMode::SetKey("openai".into()));
        for c in "jk".chars() {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            assert!(matches!(
                handle_provider_key(&mut state, &key),
                ProviderKeyOutcome::Changed
            ));
        }
        assert_eq!(state.api_key, "jk");
    }

    #[test]
    fn preset_step_j_k_still_navigate() {
        let mut state = ProviderModalState::new(ProviderModalMode::Add);
        assert_eq!(state.current_step, FormStep::Preset);
        assert_eq!(state.selected, 0);
        let down = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert!(matches!(
            handle_provider_key(&mut state, &down),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.selected, 1);
        let up = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        assert!(matches!(
            handle_provider_key(&mut state, &up),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn model_list_scrolls_with_selection() {
        let mut state = ProviderModalState::new(ProviderModalMode::Models("x".into()));
        state.models = (0..50).map(|i| format!("model-{i}")).collect();
        state.list_viewport = 10;
        state.selected = 0;
        state.scroll_offset = 0;

        // Move past the first page — scroll must follow.
        for _ in 0..15 {
            state.move_models_selection(1);
        }
        assert_eq!(state.selected, 15);
        assert!(
            state.selected >= state.scroll_offset
                && state.selected < state.scroll_offset + state.list_viewport,
            "selected {} not visible in window offset={} viewport={}",
            state.selected,
            state.scroll_offset,
            state.list_viewport
        );

        state.page_models(true);
        assert!(state.selected > 15);
        state.page_models(false);
        // Home
        let key = KeyEvent::new(KeyCode::Home, KeyModifiers::NONE);
        assert!(matches!(
            handle_provider_key(&mut state, &key),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);

        let end = KeyEvent::new(KeyCode::End, KeyModifiers::NONE);
        assert!(matches!(
            handle_provider_key(&mut state, &end),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.selected, 49);
        assert_eq!(state.scroll_offset, 40); // 50 - 10
    }

    #[test]
    fn model_filter_narrows_list_and_enter_uses_filtered() {
        let mut state = ProviderModalState::new(ProviderModalMode::SetModel("x".into()));
        state.models = vec![
            "gpt-4o".into(),
            "gpt-4o-mini".into(),
            "claude-3".into(),
            "deepseek-v3".into(),
        ];
        for c in "gpt".chars() {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            assert!(matches!(
                handle_provider_key(&mut state, &key),
                ProviderKeyOutcome::Changed
            ));
        }
        assert_eq!(state.model_filter, "gpt");
        assert_eq!(state.filtered_model_count(), 2);
        assert_eq!(state.selected_filtered_model(), Some("gpt-4o"));

        // Down moves within filtered set only.
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert!(matches!(
            handle_provider_key(&mut state, &down),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.selected_filtered_model(), Some("gpt-4o-mini"));

        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        match handle_provider_key(&mut state, &enter) {
            ProviderKeyOutcome::SwitchModel(id) => assert_eq!(id, "gpt-4o-mini"),
            other => panic!("expected SwitchModel, got {other:?}"),
        }
    }

    #[test]
    fn model_filter_esc_clears_then_back() {
        let mut state = ProviderModalState::new(ProviderModalMode::Models("x".into()));
        state.from_hub = true;
        state.models = vec!["a".into(), "b".into()];
        state.set_model_filter("a".into());
        assert_eq!(state.filtered_model_count(), 1);

        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(
            handle_provider_key(&mut state, &esc),
            ProviderKeyOutcome::Changed
        ));
        assert!(state.model_filter.is_empty());
        assert_eq!(state.filtered_model_count(), 2);

        // Second Esc leaves models list (back to actions when from_hub).
        assert!(matches!(
            handle_provider_key(&mut state, &esc),
            ProviderKeyOutcome::Changed
        ));
        assert!(matches!(state.mode, ProviderModalMode::Actions(_)));
    }

    #[test]
    fn edit_mode_steps_and_commit_allows_empty_key_when_had_key() {
        let mut state = ProviderModalState::new(ProviderModalMode::Edit("openai".into()));
        state.from_hub = true;
        state.name = "openai".into();
        state.base_url = "https://api.openai.com/v1".into();
        state.edit_had_key = true;
        state.current_step = FormStep::BaseUrl;

        // Enter advances BaseUrl → AuthScheme → ApiBackend → ApiKey
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(
            handle_provider_key(&mut state, &enter),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.current_step, FormStep::AuthScheme);
        assert!(matches!(
            handle_provider_key(&mut state, &enter),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.current_step, FormStep::ApiBackend);
        assert!(matches!(
            handle_provider_key(&mut state, &enter),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.current_step, FormStep::ApiKey);

        // Empty key + had key → Commit
        match handle_provider_key(&mut state, &enter) {
            ProviderKeyOutcome::Commit => {}
            other => panic!("expected Commit, got {other:?}"),
        }
    }

    #[test]
    fn manual_model_enter_enters_params_then_switches() {
        let mut state = ProviderModalState::new(ProviderModalMode::ManualModel("openai".into()));
        state.from_hub = true;
        for c in "gpt-4o".chars() {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            assert!(matches!(
                handle_provider_key(&mut state, &key),
                ProviderKeyOutcome::Changed
            ));
        }
        assert_eq!(state.manual_model_id, "gpt-4o");
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        // First Enter → param form (max_completion_tokens)
        assert!(matches!(
            handle_provider_key(&mut state, &enter),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(
            state.model_param_field,
            Some(ModelParamField::MaxCompletionTokens)
        );
        // Type a value
        for c in "16384".chars() {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            assert!(matches!(
                handle_provider_key(&mut state, &key),
                ProviderKeyOutcome::Changed
            ));
        }
        assert_eq!(state.max_completion_tokens, "16384");
        // Advance through remaining param fields (empty ok)
        for _ in 0..3 {
            assert!(matches!(
                handle_provider_key(&mut state, &enter),
                ProviderKeyOutcome::Changed
            ));
        }
        // Last Enter → SwitchModel
        match handle_provider_key(&mut state, &enter) {
            ProviderKeyOutcome::SwitchModel(id) => assert_eq!(id, "gpt-4o"),
            other => panic!("expected SwitchModel, got {other:?}"),
        }
    }

    #[test]
    fn actions_menu_includes_edit_manual_and_configure() {
        assert!(ProviderAction::ALL.contains(&ProviderAction::Edit));
        assert!(ProviderAction::ALL.contains(&ProviderAction::ManualModel));
        assert!(ProviderAction::ALL.contains(&ProviderAction::ConfigureModel));
        assert_eq!(ProviderAction::ALL[0], ProviderAction::Edit);
    }

    #[test]
    fn hub_esc_walks_back_actions_to_list() {
        let mut state = ProviderModalState::new(ProviderModalMode::List);
        state.providers = vec![ProviderSummary {
            name: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            auth_scheme: "bearer".into(),
            api_backend: "chat_completions".into(),
            has_key: true,
            is_current: true,
        }];
        state.go_actions("openai".into());
        assert!(matches!(state.mode, ProviderModalMode::Actions(_)));

        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(
            handle_provider_key(&mut state, &esc),
            ProviderKeyOutcome::Changed
        ));
        assert!(matches!(state.mode, ProviderModalMode::List));
    }

    #[test]
    fn hub_esc_walks_back_edit_steps_then_actions() {
        let mut state = ProviderModalState::new(ProviderModalMode::List);
        state.from_hub = true;
        state.mode = ProviderModalMode::Edit("openai".into());
        state.name = "openai".into();
        state.base_url = "https://api.openai.com/v1".into();
        state.edit_had_key = true;
        state.current_step = FormStep::ApiKey;

        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(
            handle_provider_key(&mut state, &esc),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.current_step, FormStep::ApiBackend);
        assert!(matches!(
            handle_provider_key(&mut state, &esc),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.current_step, FormStep::AuthScheme);
        assert!(matches!(
            handle_provider_key(&mut state, &esc),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.current_step, FormStep::BaseUrl);
        // First step of edit → back to actions menu (hub).
        assert!(matches!(
            handle_provider_key(&mut state, &esc),
            ProviderKeyOutcome::Changed
        ));
        assert!(matches!(state.mode, ProviderModalMode::Actions(_)));
    }

    #[test]
    fn add_form_esc_steps_back_even_without_from_hub() {
        let mut state = ProviderModalState::new(ProviderModalMode::Add);
        assert!(!state.from_hub);
        state.current_step = FormStep::ApiKey;
        state.name = "custom".into();
        state.base_url = "https://example.com".into();

        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(
            handle_provider_key(&mut state, &esc),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.current_step, FormStep::ApiBackend);
        assert!(matches!(
            handle_provider_key(&mut state, &esc),
            ProviderKeyOutcome::Changed
        ));
        assert_eq!(state.current_step, FormStep::AuthScheme);
    }

    #[test]
    fn list_esc_still_closes() {
        let mut state = ProviderModalState::new(ProviderModalMode::List);
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(
            handle_provider_key(&mut state, &esc),
            ProviderKeyOutcome::Close
        ));
    }
}
