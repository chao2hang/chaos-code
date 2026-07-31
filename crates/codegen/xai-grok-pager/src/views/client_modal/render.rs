use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::theme::Theme;
use crate::views::modal_window::{self as mw, ModalSizing, ModalWindowConfig};

use super::state::{ClientFormField, ClientModalMode, ClientModalState};

const MAX_WIDTH: u16 = 112;

fn sizing(compact: bool) -> ModalSizing {
    ModalSizing {
        width_pct: 0.72,
        max_width: MAX_WIDTH,
        min_width: 48,
        v_margin: 3,
        h_pad: 2,
        v_pad: 1,
        footer_lines: 2,
    }
    .with_compact(compact)
}

fn row_bg(theme: &Theme, selected: bool) -> Color {
    if crate::theme::cache::terminal_native_locked() || matches!(theme.bg_visual, Color::Reset) {
        if selected { Color::DarkGray } else { Color::Reset }
    } else if selected {
        theme.bg_visual
    } else {
        theme.bg_base
    }
}

pub fn render_client_modal(
    buf: &mut Buffer,
    area: Rect,
    state: &mut ClientModalState,
    compact: bool,
) {
    let theme = Theme::current();
    let (title, shortcuts): (&'static str, &'static [mw::Shortcut<'static>]) = match &state.mode {
        ClientModalMode::List => (
            "客户端选择",
            &[
                mw::Shortcut { label: "Enter 选择", clickable: false, id: 0 },
                mw::Shortcut { label: "a 新增", clickable: false, id: 1 },
                mw::Shortcut { label: "e 编辑", clickable: false, id: 2 },
                mw::Shortcut { label: "d 删除", clickable: false, id: 3 },
                mw::Shortcut { label: "s 设默认", clickable: false, id: 4 },
                mw::Shortcut { label: "Esc 关闭", clickable: false, id: 5 },
            ],
        ),
        ClientModalMode::Form { .. } => (
            "客户端配置",
            &[
                mw::Shortcut { label: "Tab 下一项", clickable: false, id: 0 },
                mw::Shortcut { label: "Shift+Tab 上一项", clickable: false, id: 1 },
                mw::Shortcut { label: "Enter 保存", clickable: false, id: 2 },
                mw::Shortcut { label: "Esc 返回", clickable: false, id: 3 },
            ],
        ),
        ClientModalMode::ConfirmDelete(_) => (
            "删除客户端",
            &[
                mw::Shortcut { label: "Enter 确认", clickable: false, id: 0 },
                mw::Shortcut { label: "Esc 取消", clickable: false, id: 1 },
            ],
        ),
    };
    let config = ModalWindowConfig {
        title,
        tabs: None,
        shortcuts,
        sizing: sizing(compact),
        fold_info: None,
    };
    let Some(content) = mw::render_modal_window(buf, area, &mut state.window, &config, &theme)
    else {
        return;
    };
    match &state.mode {
        ClientModalMode::List => render_list(buf, content.content, state, &theme),
        ClientModalMode::Form { .. } => render_form(buf, content.content, state, &theme),
        ClientModalMode::ConfirmDelete(id) => render_confirm(buf, content.content, state, id, &theme),
    }
}

fn render_list(buf: &mut Buffer, area: Rect, state: &mut ClientModalState, theme: &Theme) {
    let mut y = area.y;
    let active = state
        .current_id
        .as_deref()
        .map(|id| format!("当前会话：{id}"))
        .unwrap_or_else(|| "当前会话：未选择".into());
    Line::from(Span::styled(active, Style::default().fg(theme.accent_user))).render(
        Rect::new(area.x, y, area.width, 1),
        buf,
    );
    y += 2;
    let available = area.height.saturating_sub(4) as usize;
    state.list_viewport = available.max(1);
    state.ensure_selected_visible();
    for (index, profile) in state
        .profiles
        .iter()
        .enumerate()
        .skip(state.scroll_offset)
        .take(available.max(1))
    {
        if y >= area.y + area.height.saturating_sub(2) {
            break;
        }
        let selected = index == state.selected;
        let bg = row_bg(theme, selected);
        let row = Rect::new(area.x, y, area.width, 1);
        buf.set_style(row, Style::default().bg(bg));
        let marker = if state.current_id.as_deref() == Some(profile.id.as_str()) {
            "●"
        } else {
            " "
        };
        let kind = if ClientModalState::is_builtin(profile) { "内置" } else { "自定义" };
        let default = if state.default_id.as_deref() == Some(profile.id.as_str()) {
            " 默认"
        } else {
            ""
        };
        let line = Line::from(vec![
            Span::styled(format!("{marker} "), Style::default().fg(theme.accent_success).bg(bg)),
            Span::styled(profile.name.clone(), Style::default().fg(theme.text_primary).bg(bg).add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() })),
            Span::styled(format!("  [{}]  {} / {}  {}{}", profile.id, profile.protocol, profile.auth_scheme, kind, default), Style::default().fg(theme.gray_bright).bg(bg)),
        ]);
        line.render(row, buf);
        y += 1;
    }
    if state.profiles.is_empty() {
        Line::from(Span::styled("没有可用客户端", Style::default().fg(theme.gray_dim))).render(
            Rect::new(area.x, y, area.width, 1), buf,
        );
    }
    render_message(buf, area, state, theme);
}

fn render_form(buf: &mut Buffer, area: Rect, state: &ClientModalState, theme: &Theme) {
    let editing = state.editing_id().is_some();
    let fields = [
        (ClientFormField::Id, state.id.as_str()),
        (ClientFormField::Name, state.name.as_str()),
        (ClientFormField::Protocol, state.current_protocol()),
        (ClientFormField::AuthScheme, state.current_auth_scheme()),
        (ClientFormField::EnvKey, state.env_key.as_str()),
        (ClientFormField::ClientIdentifier, state.client_identifier.as_str()),
    ];
    let mut y = area.y;
    let intro = if editing { "修改自定义客户端（ID 保持不变）" } else { "新增自定义客户端（不保存 API Key）" };
    Line::from(Span::styled(intro, Style::default().fg(theme.gray_bright))).render(
        Rect::new(area.x, y, area.width, 1), buf,
    );
    y += 2;
    for (field, value) in fields {
        if editing && field == ClientFormField::Id {
            continue;
        }
        let active = state.form_field == field;
        let label_style = if active { Style::default().fg(theme.accent_user).add_modifier(Modifier::BOLD) } else { Style::default().fg(theme.gray_bright) };
        let value_style = if active { Style::default().fg(theme.text_primary).add_modifier(Modifier::BOLD) } else { Style::default().fg(theme.text_primary) };
        let suffix = if active { "▌" } else { "" };
        Line::from(vec![
            Span::styled(format!("{: <10}", field.label()), label_style),
            Span::styled(format!("{}{}", if value.is_empty() { "<空>" } else { value }, suffix), value_style),
        ]).render(Rect::new(area.x, y, area.width, 1), buf);
        y += 1;
    }
    Line::from(Span::styled("认证为 none 时环境变量可留空；仅保存变量名，不保存密钥。", Style::default().fg(theme.gray_dim))).render(Rect::new(area.x, y + 1, area.width, 1), buf);
    render_message(buf, area, state, theme);
}

fn render_confirm(buf: &mut Buffer, area: Rect, state: &ClientModalState, id: &str, theme: &Theme) {
    let lines = [
        format!("确定删除自定义客户端 \"{id}\"？"),
        "这会清除默认设置和模型级客户端引用。".to_owned(),
        "按 Enter 确认，Esc 取消。".to_owned(),
    ];
    for (offset, line) in lines.iter().enumerate() {
        Line::from(Span::styled(line.clone(), if offset == 0 { Style::default().fg(theme.accent_error).add_modifier(Modifier::BOLD) } else { Style::default().fg(theme.gray_bright) })).render(Rect::new(area.x, area.y + offset as u16 * 2, area.width, 1), buf);
    }
    render_message(buf, area, state, theme);
}

fn render_message(buf: &mut Buffer, area: Rect, state: &ClientModalState, theme: &Theme) {
    let message = state.error.as_deref().or(state.success.as_deref());
    let Some(message) = message else { return };
    let style = if state.error.is_some() { Style::default().fg(theme.accent_error) } else { Style::default().fg(theme.accent_success) };
    let y = area.y + area.height.saturating_sub(1);
    Line::from(Span::styled(message.to_owned(), style)).render(Rect::new(area.x, y, area.width, 1), buf);
}
