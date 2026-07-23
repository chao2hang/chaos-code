//! Provider modal keyboard input handling.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::state::{
    API_BACKENDS, AUTH_SCHEMES, FormStep, ProviderAction, ProviderKeyOutcome, ProviderModalMode,
    ProviderModalState, PROVIDER_PRESETS,
};

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

    // Ctrl+V 粘贴 — 由 handle_modal_paste 处理，这里跳过
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('v')) {
        return ProviderKeyOutcome::Unchanged;
    }

    if state.success.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                let msg = state.success.take();
                // Hub：成功后回到列表；深链：关闭
                if state.from_hub {
                    // 添加成功后回到列表；设 key 后回到操作菜单
                    if matches!(state.mode, ProviderModalMode::Add) {
                        state.go_list();
                    } else if let ProviderModalMode::SetKey(name) = &state.mode {
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
        ProviderModalMode::SetKey(_) => handle_set_key(state, key),
        ProviderModalMode::Models(_) => handle_models(state, key),
        ProviderModalMode::SetModel(_) => handle_set_model(state, key),
    }
}

/// 处理粘贴事件——将文本粘贴到当前输入字段。
pub fn handle_provider_paste(state: &mut ProviderModalState, text: &str) -> ProviderKeyOutcome {
    if state.success.is_some() {
        return ProviderKeyOutcome::Unchanged;
    }
    match &state.mode {
        ProviderModalMode::Add => {
            let field = match state.current_step {
                FormStep::Name => &mut state.name,
                FormStep::BaseUrl => &mut state.base_url,
                FormStep::ApiKey => &mut state.api_key,
                _ => return ProviderKeyOutcome::Unchanged,
            };
            field.push_str(text);
            state.error = None;
            ProviderKeyOutcome::Changed
        }
        ProviderModalMode::SetKey(_) => {
            state.api_key.push_str(text);
            state.error = None;
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
        KeyCode::Up | KeyCode::Char('k') => {
            if state.current_step == FormStep::Preset {
                if state.selected > 0 {
                    state.selected -= 1;
                }
                ProviderKeyOutcome::Changed
            } else {
                ProviderKeyOutcome::Unchanged
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.current_step == FormStep::Preset {
                let max = PROVIDER_PRESETS.len(); // 自定义额外占一行
                if state.selected < max {
                    state.selected += 1;
                }
                ProviderKeyOutcome::Changed
            } else {
                ProviderKeyOutcome::Unchanged
            }
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
                    state.current_step = FormStep::Name;
                    state.name.clear();
                    state.base_url.clear();
                } else {
                    return ProviderKeyOutcome::Unchanged;
                }
                state.error = None;
                ProviderKeyOutcome::Changed
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
                FormStep::Preset => ProviderKeyOutcome::Unchanged,
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
            *idx = if *idx == 0 { choice_count - 1 } else { *idx - 1 };
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
                .unwrap_or(ProviderAction::SetKey);
            match action {
                ProviderAction::SetKey => state.go_set_key(name),
                ProviderAction::Models | ProviderAction::Refresh => state.go_models(name),
                ProviderAction::SetModel => state.go_set_model(name),
            }
            ProviderKeyOutcome::Changed
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

fn handle_models(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    let len = state.models.len();
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
        // 在查看列表里 Enter 也可进入切换（hub 体验）
        KeyCode::Enter if state.from_hub && len > 0 => {
            let model_id = state.models[state.selected].clone();
            ProviderKeyOutcome::SwitchModel(model_id)
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}

fn handle_set_model(state: &mut ProviderModalState, key: &KeyEvent) -> ProviderKeyOutcome {
    let len = state.models.len();
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
            if len == 0 {
                return ProviderKeyOutcome::Unchanged;
            }
            let model_id = state.models[state.selected].clone();
            ProviderKeyOutcome::SwitchModel(model_id)
        }
        _ => ProviderKeyOutcome::Unchanged,
    }
}
