use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::state::{
    AUTH_SCHEMES, ClientFormField, ClientKeyOutcome, ClientModalMode, ClientModalState, PROTOCOLS,
};

pub fn handle_client_key(state: &mut ClientModalState, key: &KeyEvent) -> ClientKeyOutcome {
    if key.kind == KeyEventKind::Release {
        return ClientKeyOutcome::Unchanged;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('g'))
    {
        return ClientKeyOutcome::Close;
    }

    if state.success.is_some() {
        if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
            state.success = None;
            return ClientKeyOutcome::Changed;
        }
        state.success = None;
        return ClientKeyOutcome::Changed;
    }

    match state.mode.clone() {
        ClientModalMode::List => handle_list(state, key),
        ClientModalMode::Form { editing_id } => handle_form(state, key, editing_id),
        ClientModalMode::ConfirmDelete(id) => handle_confirm_delete(state, key, id),
    }
}

pub fn handle_client_paste(state: &mut ClientModalState, text: &str) -> ClientKeyOutcome {
    if !matches!(state.mode, ClientModalMode::Form { .. }) || state.success.is_some() {
        return ClientKeyOutcome::Unchanged;
    }
    let value = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned();
    if value.is_empty() || !state.set_pasted_text(value) {
        ClientKeyOutcome::Unchanged
    } else {
        ClientKeyOutcome::Changed
    }
}

fn handle_list(state: &mut ClientModalState, key: &KeyEvent) -> ClientKeyOutcome {
    match key.code {
        KeyCode::Esc => ClientKeyOutcome::Close,
        KeyCode::Up | KeyCode::Char('k') => {
            state.move_selection(-1);
            ClientKeyOutcome::Changed
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.move_selection(1);
            ClientKeyOutcome::Changed
        }
        KeyCode::Char('a') if key.modifiers.is_empty() => {
            state.start_add();
            ClientKeyOutcome::Changed
        }
        KeyCode::Char('e') if key.modifiers.is_empty() => {
            state.start_edit();
            ClientKeyOutcome::Changed
        }
        KeyCode::Char('d') if key.modifiers.is_empty() => {
            let Some(profile) = state.selected_profile().cloned() else {
                return ClientKeyOutcome::Unchanged;
            };
            if ClientModalState::is_builtin(&profile) {
                state.error = Some("内置客户端不能删除".into());
                return ClientKeyOutcome::Changed;
            }
            state.mode = ClientModalMode::ConfirmDelete(profile.id);
            ClientKeyOutcome::Changed
        }
        KeyCode::Char('s') if key.modifiers.is_empty() => state
            .selected_profile()
            .map(|profile| ClientKeyOutcome::SetDefault(profile.id.clone()))
            .unwrap_or(ClientKeyOutcome::Unchanged),
        KeyCode::Char('r') if key.modifiers.is_empty() => {
            state.reload_profiles();
            ClientKeyOutcome::Changed
        }
        KeyCode::Enter => state
            .selected_profile()
            .cloned()
            .map(ClientKeyOutcome::Select)
            .unwrap_or(ClientKeyOutcome::Unchanged),
        _ => ClientKeyOutcome::Unchanged,
    }
}

fn handle_form(
    state: &mut ClientModalState,
    key: &KeyEvent,
    editing_id: Option<String>,
) -> ClientKeyOutcome {
    let editing = editing_id.is_some();
    match key.code {
        KeyCode::Esc => {
            state.mode = ClientModalMode::List;
            state.error = None;
            ClientKeyOutcome::Changed
        }
        KeyCode::Backspace => {
            if state.pop_text() {
                ClientKeyOutcome::Changed
            } else {
                ClientKeyOutcome::Unchanged
            }
        }
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(previous) = state.form_field.previous(editing) {
                state.form_field = previous;
            }
            ClientKeyOutcome::Changed
        }
        KeyCode::BackTab => {
            if let Some(previous) = state.form_field.previous(editing) {
                state.form_field = previous;
            }
            ClientKeyOutcome::Changed
        }
        KeyCode::Tab | KeyCode::Enter => {
            if let Some(next) = state.form_field.next(editing) {
                state.form_field = next;
                return ClientKeyOutcome::Changed;
            }
            ClientKeyOutcome::Commit {
                profile: state.build_profile(),
                editing_id,
            }
        }
        KeyCode::Up | KeyCode::Left => cycle_choice(state, -1),
        KeyCode::Down | KeyCode::Right => cycle_choice(state, 1),
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.push_text(c);
            ClientKeyOutcome::Changed
        }
        _ => ClientKeyOutcome::Unchanged,
    }
}

fn cycle_choice(state: &mut ClientModalState, delta: isize) -> ClientKeyOutcome {
    match state.form_field {
        ClientFormField::Protocol => {
            state.protocol_idx = cycle_index(state.protocol_idx, PROTOCOLS.len(), delta);
            ClientKeyOutcome::Changed
        }
        ClientFormField::AuthScheme => {
            state.auth_scheme_idx = cycle_index(state.auth_scheme_idx, AUTH_SCHEMES.len(), delta);
            ClientKeyOutcome::Changed
        }
        _ => ClientKeyOutcome::Unchanged,
    }
}

fn cycle_index(index: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (index as isize + delta).rem_euclid(len as isize) as usize
}

fn handle_confirm_delete(
    state: &mut ClientModalState,
    key: &KeyEvent,
    id: String,
) -> ClientKeyOutcome {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') => {
            state.mode = ClientModalMode::List;
            ClientKeyOutcome::Changed
        }
        KeyCode::Enter | KeyCode::Char('y') => ClientKeyOutcome::Delete(id),
        _ => ClientKeyOutcome::Unchanged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn list_enter_selects_the_highlighted_profile() {
        let mut state = ClientModalState::new(None);
        state.profiles = vec![xai_grok_shell::agent::client_profiles::by_id("codex").unwrap()];
        assert!(matches!(
            handle_client_key(&mut state, &key(KeyCode::Enter)),
            ClientKeyOutcome::Select(_)
        ));
    }

    #[test]
    fn builtins_cannot_be_edited_or_deleted() {
        let mut state = ClientModalState::new(None);
        state.profiles = vec![xai_grok_shell::agent::client_profiles::by_id("codex").unwrap()];
        assert!(matches!(
            handle_client_key(&mut state, &key(KeyCode::Char('e'))),
            ClientKeyOutcome::Changed
        ));
        assert!(matches!(state.mode, ClientModalMode::List));
        assert!(matches!(
            handle_client_key(&mut state, &key(KeyCode::Char('d'))),
            ClientKeyOutcome::Changed
        ));
        assert!(matches!(state.mode, ClientModalMode::List));
    }

    #[test]
    fn add_form_advances_and_commits_without_a_secret_field() {
        let mut state = ClientModalState::new(None);
        state.start_add();
        for c in "my-client".chars() {
            handle_client_key(&mut state, &key(KeyCode::Char(c)));
        }
        handle_client_key(&mut state, &key(KeyCode::Tab)); // name
        for c in "My Client".chars() {
            handle_client_key(&mut state, &key(KeyCode::Char(c)));
        }
        handle_client_key(&mut state, &key(KeyCode::Tab)); // protocol
        handle_client_key(&mut state, &key(KeyCode::Tab)); // auth
        handle_client_key(&mut state, &key(KeyCode::Tab)); // environment variable
        for c in "MY_CLIENT_API_KEY".chars() {
            handle_client_key(&mut state, &key(KeyCode::Char(c)));
        }
        handle_client_key(&mut state, &key(KeyCode::Tab)); // client identifier
        for c in "my-client".chars() {
            handle_client_key(&mut state, &key(KeyCode::Char(c)));
        }
        handle_client_key(&mut state, &key(KeyCode::Tab)); // user agent (spaces allowed)
        for c in "WorkBuddy/5.3.5 WorkBuddy/5.3.5 CLI/2.115.0".chars() {
            handle_client_key(&mut state, &key(KeyCode::Char(c)));
        }
        let outcome = handle_client_key(&mut state, &key(KeyCode::Enter));
        assert_eq!(state.env_key, "MY_CLIENT_API_KEY");
        assert_eq!(state.client_identifier, "my-client");
        assert_eq!(
            state.user_agent,
            "WorkBuddy/5.3.5 WorkBuddy/5.3.5 CLI/2.115.0"
        );
        assert!(matches!(
            outcome,
            ClientKeyOutcome::Commit {
                ref profile,
                editing_id: None,
            } if profile.id == "my-client"
                && profile.name == "My Client"
                && profile.env_key == "MY_CLIENT_API_KEY"
                && profile.user_agent.as_deref()
                    == Some("WorkBuddy/5.3.5 WorkBuddy/5.3.5 CLI/2.115.0")
        ));
    }
}
