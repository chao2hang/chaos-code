//! Provider modal keyboard input handling.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::state::{
    API_BACKENDS, AUTH_SCHEMES, FormStep, ProviderAction, ProviderKeyOutcome, ProviderModalMode,
    ProviderModalState, PROVIDER_PRESETS,
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
    line.chars()
        .filter(|c| !matches!(c, '\r' | '\n'))
        .collect()
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
    let cleaned = sanitize_provider_field(text);
    if cleaned.is_empty() {
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
                let max = PROVIDER_PRESETS.len(); // 自定义额外占一行
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
                && state.current_step == FormStep::Preset =>
        {
            let max = PROVIDER_PRESETS.len();
            if state.selected < max {
                state.selected += 1;
            }
            ProviderKeyOutcome::Changed
        }
        KeyCode::Char('k')
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && state.current_step == FormStep::Preset =>
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
        KeyCode::Enter if allow_enter_switch => {
            match state.selected_filtered_model() {
                Some(id) => ProviderKeyOutcome::SwitchModel(id.to_string()),
                None => ProviderKeyOutcome::Unchanged,
            }
        }
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
    use super::*;

    #[test]
    fn sanitize_strips_windows_crlf_and_bom() {
        assert_eq!(sanitize_provider_field("sk-abc123\r\n"), "sk-abc123");
        assert_eq!(sanitize_provider_field("\u{feff}sk-abc123\r"), "sk-abc123");
        assert_eq!(sanitize_provider_field("  sk-abc123  \n"), "sk-abc123");
    }

    #[test]
    fn sanitize_keeps_first_non_empty_line() {
        assert_eq!(
            sanitize_provider_field("sk-first\nsk-second\n"),
            "sk-first"
        );
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
}
