//! Provider modal rendering.
//!
//! Visual chrome matches the rest of the pager modals (settings / pickers):
//! shared [`ModalSizing`] (70% × max 110, `v_margin: 3`), square
//! `gray_dim` border via [`mw::render_modal_window`], and full-row
//! `bg_visual` selection — not the older accent-fg + `▸` list style.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::Theme;
use crate::views::modal_window::{self as mw, ModalWindowConfig, ModalSizing};

use super::state::{
    API_BACKENDS, AUTH_SCHEMES, FormStep, ProviderAction, ProviderModalMode, ProviderModalState,
    PROVIDER_PRESETS,
};

/// Same footprint as the settings modal: ~70% width, max 110 cols.
const STANDARD_MAX_WIDTH: u16 = 110;

/// Shared sizing for every provider-modal surface (list / form / menus).
fn provider_sizing(compact: bool) -> ModalSizing {
    ModalSizing {
        width_pct: 0.70,
        max_width: STANDARD_MAX_WIDTH,
        min_width: 44,
        v_margin: 3,
        h_pad: 2,
        v_pad: 1,
        footer_lines: 2,
    }
    .with_compact(compact)
}

/// Row background: selected → `bg_visual` (DarkGray on terminal-native),
/// otherwise modal base. Mirrors settings_list_row_bg / picker rows.
fn list_row_bg(theme: &Theme, selected: bool) -> Color {
    if crate::theme::cache::terminal_native_locked() || matches!(theme.bg_visual, Color::Reset) {
        return if selected {
            Color::DarkGray
        } else {
            Color::Reset
        };
    }
    if selected {
        theme.bg_visual
    } else {
        theme.bg_base
    }
}

/// Paint a selectable list row with full-width bg + bold primary text.
/// Selection is the band, not a `▸` glyph (matches import-claude / picker).
fn paint_list_row(buf: &mut Buffer, area: Rect, y: u16, selected: bool, theme: &Theme, spans: Vec<Span<'_>>) {
    let bg = list_row_bg(theme, selected);
    let row = Rect {
        x: area.x,
        y,
        width: area.width,
        height: 1,
    };
    buf.set_style(row, Style::default().bg(bg));
    let line = Line::from(
        spans
            .into_iter()
            .map(|s| {
                let mut style = s.style.bg(bg);
                if selected {
                    style = style.add_modifier(Modifier::BOLD);
                }
                Span::styled(s.content, style)
            })
            .collect::<Vec<_>>(),
    );
    line.render(row, buf);
}

/// 渲染 Provider 模态框。
pub fn render_provider_modal(
    buf: &mut Buffer,
    area: Rect,
    state: &mut ProviderModalState,
    compact: bool,
) {
    let theme = Theme::current();
    let mode = state.mode.clone();
    match &mode {
        ProviderModalMode::SetKey(name) => render_set_key(buf, area, state, name, &theme, compact),
        ProviderModalMode::ManualModel(name) => {
            render_manual_model(buf, area, state, name, &theme, compact)
        }
        ProviderModalMode::Models(name) => render_models(buf, area, state, name, &theme, compact),
        ProviderModalMode::SetModel(name) => {
            render_set_model(buf, area, state, name, &theme, compact)
        }
        ProviderModalMode::Actions(name) => render_actions(buf, area, state, name, &theme, compact),
        ProviderModalMode::List | ProviderModalMode::Add | ProviderModalMode::Edit(_) => {
            let title = match &mode {
                ProviderModalMode::Add => "添加渠道",
                ProviderModalMode::Edit(name) => {
                    // title is &'static in ModalWindowConfig — use stack buffer via leak-free path:
                    // render_edit uses its own window; here we still need a static-ish title.
                    // Use a fixed label; name shown in form body.
                    let _ = name;
                    "编辑渠道"
                }
                _ => "渠道管理",
            };
            let sizing = provider_sizing(compact);
            let shortcuts: &[mw::Shortcut<'static>] = match &mode {
                ProviderModalMode::List => &[
                    mw::Shortcut {
                        label: "Enter 选择",
                        clickable: false,
                        id: 0,
                    },
                    mw::Shortcut {
                        label: "a 添加",
                        clickable: false,
                        id: 1,
                    },
                    mw::Shortcut {
                        label: "Esc 关闭",
                        clickable: false,
                        id: 2,
                    },
                ],
                _ => &[
                    mw::Shortcut {
                        label: "Enter 下一步",
                        clickable: false,
                        id: 0,
                    },
                    mw::Shortcut {
                        label: "Esc 返回",
                        clickable: false,
                        id: 1,
                    },
                ],
            };
            let config = ModalWindowConfig {
                title,
                tabs: None,
                shortcuts,
                sizing,
                fold_info: None,
            };

            let Some(mca) = mw::render_modal_window(buf, area, &mut state.window, &config, &theme)
            else {
                return;
            };

            let content = mca.content;
            match &state.mode {
                ProviderModalMode::Add => render_add_form(buf, content, state, &theme),
                ProviderModalMode::Edit(_) => render_edit_form(buf, content, state, &theme),
                ProviderModalMode::List => render_list_content(buf, content, state, &theme),
                _ => {}
            }
        }
    }
}

// ── /provider add 表单 ──────────────────────────────────────────────────────

fn render_add_form(buf: &mut Buffer, area: Rect, state: &ProviderModalState, theme: &Theme) {
    let label_style = Style::default().fg(theme.gray_bright);
    let value_style = Style::default().fg(theme.text_primary);
    let active_label_style = Style::default()
        .fg(theme.accent_user)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(theme.gray_dim);

    let mut y = area.y;

    // ── 预设选择步骤 ──
    if state.current_step == FormStep::Preset {
        let title = Line::from(Span::styled(
            "选择预设渠道（↑/↓ 选择，Enter 确认）:",
            active_label_style,
        ));
        title.render(Rect::new(area.x, y, area.width, 1), buf);
        y += 1;

        for (i, p) in PROVIDER_PRESETS.iter().enumerate() {
            if y >= area.y + area.height {
                break;
            }
            let selected = i == state.selected;
            paint_list_row(
                buf,
                area,
                y,
                selected,
                theme,
                vec![
                    Span::styled(
                        format!("  {}", p.display),
                        Style::default().fg(theme.text_primary),
                    ),
                    Span::styled(format!("  ({})", p.base_url), Style::default().fg(theme.gray_dim)),
                ],
            );
            y += 1;
        }
        // 自定义选项
        let custom_idx = PROVIDER_PRESETS.len();
        if y < area.y + area.height {
            let selected = state.selected == custom_idx;
            paint_list_row(
                buf,
                area,
                y,
                selected,
                theme,
                vec![Span::styled(
                    "  自定义",
                    Style::default().fg(theme.text_primary),
                )],
            );
            y += 1;
        }
    } else {
        // ── 表单字段 ──
        let steps = [
            FormStep::Name,
            FormStep::BaseUrl,
            FormStep::AuthScheme,
            FormStep::ApiBackend,
            FormStep::ApiKey,
        ];

        for step in &steps {
            if y >= area.y + area.height {
                break;
            }
            let is_current = *step == state.current_step;
            let lbl_style = if is_current {
                active_label_style
            } else {
                label_style
            };

            let (label, value): (&str, String) = match step {
                FormStep::Name => ("名称", state.name.clone()),
                FormStep::BaseUrl => ("Base URL", state.base_url.clone()),
                FormStep::AuthScheme => ("认证方式", AUTH_SCHEMES[state.auth_scheme_idx].into()),
                FormStep::ApiBackend => ("API 后端", API_BACKENDS[state.api_backend_idx].into()),
                FormStep::ApiKey => ("API Key", mask_key(&state.api_key)),
                FormStep::Preset => continue,
            };

            let prefix = format!("{}: ", label);
            let prefix_w = prefix.width() as u16;
            let line = Line::from(vec![
                Span::styled(prefix, lbl_style),
                Span::styled(value, value_style),
            ]);
            line.render(Rect::new(area.x, y, area.width, 1), buf);

            if is_current {
                let field_len = match step {
                    FormStep::Name => state.name.width(),
                    FormStep::BaseUrl => state.base_url.width(),
                    FormStep::AuthScheme => AUTH_SCHEMES[state.auth_scheme_idx].width(),
                    FormStep::ApiBackend => API_BACKENDS[state.api_backend_idx].width(),
                    FormStep::ApiKey => mask_key(&state.api_key).width(),
                    FormStep::Preset => 0,
                };
                let cursor_x = area.x + prefix_w + field_len as u16;
                if cursor_x < area.x + area.width {
                    if let Some(cell) = buf.cell_mut((cursor_x, y)) {
                        cell.set_style(Style::default().fg(theme.bg_dark).bg(theme.text_primary));
                    }
                }
                if matches!(step, FormStep::AuthScheme | FormStep::ApiBackend) {
                    let hint = "  ←/→ 或 ↑/↓ 切换";
                    let hint_x = area.x + prefix_w + field_len as u16 + 2;
                    if hint_x + hint.width() as u16 <= area.x + area.width {
                        buf.set_string(hint_x, y, hint, dim_style);
                    }
                }
            }
            y += 1;
        }
    }

    // 错误消息
    if let Some(err) = &state.error {
        if y < area.y + area.height {
            let err_line = Line::from(Span::styled(
                format!("✗ {err}"),
                Style::default().fg(theme.accent_error),
            ));
            err_line.render(Rect::new(area.x, y, area.width, 1), buf);
            y += 1;
        }
    }

    // 成功消息
    if let Some(succ) = &state.success {
        if y < area.y + area.height {
            let succ_line = Line::from(Span::styled(
                format!("✓ {succ}"),
                Style::default().fg(theme.accent_success),
            ));
            succ_line.render(Rect::new(area.x, y, area.width, 1), buf);
        }
    }
}

// ── 编辑渠道表单 ───────────────────────────────────────────────────────────

fn render_edit_form(buf: &mut Buffer, area: Rect, state: &ProviderModalState, theme: &Theme) {
    let label_style = Style::default().fg(theme.gray_bright);
    let value_style = Style::default().fg(theme.text_primary);
    let active_label_style = Style::default()
        .fg(theme.accent_user)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(theme.gray_dim);

    let mut y = area.y;

    // 渠道名只读展示
    if y < area.y + area.height {
        let line = Line::from(vec![
            Span::styled("名称: ", label_style),
            Span::styled(state.name.as_str(), value_style),
            Span::styled("  (不可改)", dim_style),
        ]);
        line.render(Rect::new(area.x, y, area.width, 1), buf);
        y += 1;
    }

    let steps = [
        FormStep::BaseUrl,
        FormStep::AuthScheme,
        FormStep::ApiBackend,
        FormStep::ApiKey,
    ];

    for step in &steps {
        if y >= area.y + area.height {
            break;
        }
        let is_current = *step == state.current_step;
        let lbl_style = if is_current {
            active_label_style
        } else {
            label_style
        };

        let (label, value): (&str, String) = match step {
            FormStep::BaseUrl => ("Base URL", state.base_url.clone()),
            FormStep::AuthScheme => ("认证方式", AUTH_SCHEMES[state.auth_scheme_idx].into()),
            FormStep::ApiBackend => ("API 后端", API_BACKENDS[state.api_backend_idx].into()),
            FormStep::ApiKey => {
                let display = if state.api_key.is_empty() {
                    if state.edit_had_key {
                        "（留空保留原密钥）".into()
                    } else {
                        String::new()
                    }
                } else {
                    mask_key(&state.api_key)
                };
                ("API Key", display)
            }
            FormStep::Preset | FormStep::Name => continue,
        };

        let prefix = format!("{label}: ");
        let prefix_w = prefix.width() as u16;
        let line = Line::from(vec![
            Span::styled(prefix, lbl_style),
            Span::styled(
                value.clone(),
                if state.api_key.is_empty()
                    && *step == FormStep::ApiKey
                    && state.edit_had_key
                    && !is_current
                {
                    dim_style
                } else if state.api_key.is_empty()
                    && *step == FormStep::ApiKey
                    && state.edit_had_key
                {
                    dim_style
                } else {
                    value_style
                },
            ),
        ]);
        line.render(Rect::new(area.x, y, area.width, 1), buf);

        if is_current {
            let field_len = match step {
                FormStep::BaseUrl => state.base_url.width(),
                FormStep::AuthScheme => AUTH_SCHEMES[state.auth_scheme_idx].width(),
                FormStep::ApiBackend => API_BACKENDS[state.api_backend_idx].width(),
                FormStep::ApiKey => {
                    if state.api_key.is_empty() && state.edit_had_key {
                        "（留空保留原密钥）".width()
                    } else {
                        mask_key(&state.api_key).width()
                    }
                }
                FormStep::Preset | FormStep::Name => 0,
            };
            let cursor_x = area.x + prefix_w + field_len as u16;
            if cursor_x < area.x + area.width {
                if let Some(cell) = buf.cell_mut((cursor_x, y)) {
                    cell.set_style(Style::default().fg(theme.bg_dark).bg(theme.text_primary));
                }
            }
            if matches!(step, FormStep::AuthScheme | FormStep::ApiBackend) {
                let hint = "  ←/→ 或 ↑/↓ 切换";
                let hint_x = area.x + prefix_w + field_len as u16 + 2;
                if hint_x + hint.width() as u16 <= area.x + area.width {
                    buf.set_string(hint_x, y, hint, dim_style);
                }
            }
        }
        y += 1;
    }

    if let Some(err) = &state.error {
        if y < area.y + area.height {
            let err_line = Line::from(Span::styled(
                format!("✗ {err}"),
                Style::default().fg(theme.accent_error),
            ));
            err_line.render(Rect::new(area.x, y, area.width, 1), buf);
            y += 1;
        }
    }

    if let Some(succ) = &state.success {
        if y < area.y + area.height {
            let succ_line = Line::from(Span::styled(
                format!("✓ {succ}"),
                Style::default().fg(theme.accent_success),
            ));
            succ_line.render(Rect::new(area.x, y, area.width, 1), buf);
        }
    }
}

// ── 手动输入模型 ID ────────────────────────────────────────────────────────

fn render_manual_model(
    buf: &mut Buffer,
    area: Rect,
    state: &mut ProviderModalState,
    name: &str,
    theme: &Theme,
    compact: bool,
) {
    let sizing = provider_sizing(compact);
    let back = if state.from_hub {
        "Esc 返回"
    } else {
        "Esc 取消"
    };
    let shortcuts: &[mw::Shortcut<'static>] = &[
        mw::Shortcut {
            label: "Enter 确认",
            clickable: false,
            id: 0,
        },
        mw::Shortcut {
            label: back,
            clickable: false,
            id: 1,
        },
    ];
    let config = ModalWindowConfig {
        title: "手动输入模型",
        tabs: None,
        shortcuts,
        sizing,
        fold_info: None,
    };

    let Some(mca) = mw::render_modal_window(buf, area, &mut state.window, &config, theme) else {
        return;
    };
    let content = mca.content;
    let mut y = content.y;

    let label_style = Style::default().fg(theme.gray_bright);
    let value_style = Style::default().fg(theme.text_primary);
    let dim_style = Style::default().fg(theme.gray_dim);

    let name_line = Line::from(vec![
        Span::styled("渠道: ", label_style),
        Span::styled(name, value_style),
    ]);
    name_line.render(Rect::new(content.x, y, content.width, 1), buf);
    y += 1;

    if y < content.y + content.height {
        let hint = Line::from(Span::styled(
            "输入上游模型 ID（如 gpt-4o、claude-sonnet-4-20250514）",
            dim_style,
        ));
        hint.render(Rect::new(content.x, y, content.width, 1), buf);
        y += 1;
    }

    let key_label = "模型 ID: ";
    let id = state.manual_model_id.as_str();
    let key_line = Line::from(vec![
        Span::styled(key_label, label_style),
        Span::styled(id, value_style),
    ]);
    key_line.render(Rect::new(content.x, y, content.width, 1), buf);

    let prefix_w = key_label.width() as u16;
    let field_w = id.width() as u16;
    let cursor_x = content.x + prefix_w + field_w;
    if cursor_x < content.x + content.width {
        if let Some(cell) = buf.cell_mut((cursor_x, y)) {
            cell.set_style(Style::default().fg(theme.bg_dark).bg(theme.text_primary));
        }
    }
    y += 1;

    if let Some(err) = &state.error {
        if y < content.y + content.height {
            let err_line = Line::from(Span::styled(
                format!("✗ {err}"),
                Style::default().fg(theme.accent_error),
            ));
            err_line.render(Rect::new(content.x, y, content.width, 1), buf);
            y += 1;
        }
    }

    if let Some(succ) = &state.success {
        if y < content.y + content.height {
            let succ_line = Line::from(Span::styled(
                format!("✓ {succ}"),
                Style::default().fg(theme.accent_success),
            ));
            succ_line.render(Rect::new(content.x, y, content.width, 1), buf);
        }
    }
}

// ── /provider set-key <name> ───────────────────────────────────────────────

fn render_set_key(
    buf: &mut Buffer,
    area: Rect,
    state: &mut ProviderModalState,
    name: &str,
    theme: &Theme,
    compact: bool,
) {
    let sizing = provider_sizing(compact);
    let back = if state.from_hub {
        "Esc 返回"
    } else {
        "Esc 取消"
    };
    let shortcuts: &[mw::Shortcut<'static>] = &[
        mw::Shortcut {
            label: "Enter 确认",
            clickable: false,
            id: 0,
        },
        mw::Shortcut {
            label: back,
            clickable: false,
            id: 1,
        },
    ];
    let config = ModalWindowConfig {
        title: "设置 API Key",
        tabs: None,
        shortcuts,
        sizing,
        fold_info: None,
    };

    let Some(mca) = mw::render_modal_window(buf, area, &mut state.window, &config, theme) else {
        return;
    };
    let content = mca.content;
    let mut y = content.y;

    let label_style = Style::default().fg(theme.gray_bright);
    let value_style = Style::default().fg(theme.text_primary);

    let name_line = Line::from(vec![
        Span::styled("渠道: ", label_style),
        Span::styled(name, value_style),
    ]);
    name_line.render(Rect::new(content.x, y, content.width, 1), buf);
    y += 1;

    let key_label = "API Key: ";
    let masked = mask_key(&state.api_key);
    let key_line = Line::from(vec![
        Span::styled(key_label, label_style),
        Span::styled(&masked, value_style),
    ]);
    key_line.render(Rect::new(content.x, y, content.width, 1), buf);

    // 光标
    let prefix_w = key_label.width() as u16;
    let field_w = masked.width() as u16;
    let cursor_x = content.x + prefix_w + field_w;
    if cursor_x < content.x + content.width {
        if let Some(cell) = buf.cell_mut((cursor_x, y)) {
            cell.set_style(Style::default().fg(theme.bg_dark).bg(theme.text_primary));
        }
    }
    y += 1;

    if let Some(err) = &state.error {
        if y < content.y + content.height {
            let err_line = Line::from(Span::styled(
                format!("✗ {err}"),
                Style::default().fg(theme.accent_error),
            ));
            err_line.render(Rect::new(content.x, y, content.width, 1), buf);
            y += 1;
        }
    }

    if let Some(succ) = &state.success {
        if y < content.y + content.height {
            let succ_line = Line::from(Span::styled(
                format!("✓ {succ}"),
                Style::default().fg(theme.accent_success),
            ));
            succ_line.render(Rect::new(content.x, y, content.width, 1), buf);
        }
    }
}

// ── 渠道列表 hub ───────────────────────────────────────────────────────────

fn render_list_content(buf: &mut Buffer, area: Rect, state: &ProviderModalState, theme: &Theme) {
    let header_style = Style::default()
        .fg(theme.text_secondary)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(theme.gray_dim);

    let mut y = area.y;

    if state.providers.is_empty() {
        let line = Line::from(Span::styled(
            "尚未配置任何渠道。选择下方「+ 添加渠道」开始。",
            Style::default().fg(theme.gray),
        ));
        line.render(Rect::new(area.x, y, area.width, 1), buf);
        y += 2;
    } else {
        // 表头（非可选行，不铺选中底）
        let header = Line::from(vec![
            Span::styled("  名称", header_style),
            Span::styled("  ", header_style),
            Span::styled("Base URL", header_style),
            Span::styled("  ", header_style),
            Span::styled("认证", header_style),
            Span::styled("  ", header_style),
            Span::styled("后端", header_style),
            Span::styled("  ", header_style),
            Span::styled("密钥", header_style),
        ]);
        header.render(Rect::new(area.x, y, area.width, 1), buf);
        y += 1;

        // 分隔线（与 modal chrome 一致的 gray_dim ─）
        if y < area.y + area.height {
            let div: String = std::iter::repeat_n('─', area.width as usize).collect();
            buf.set_string(area.x, y, &div, dim_style);
            y += 1;
        }

        for (i, p) in state.providers.iter().enumerate() {
            if y >= area.y + area.height {
                break;
            }
            let selected = i == state.selected;
            let marker = if p.is_current { " *" } else { "  " };
            let key_status = if p.has_key { "✓" } else { "✗" };
            paint_list_row(
                buf,
                area,
                y,
                selected,
                theme,
                vec![
                    Span::styled(
                        format!("  {}{}", p.name, marker),
                        Style::default().fg(theme.text_primary),
                    ),
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        truncate_str(&p.base_url, 28),
                        Style::default().fg(theme.text_primary),
                    ),
                    Span::styled("  ", Style::default()),
                    Span::styled(p.auth_scheme.as_str(), Style::default().fg(theme.gray)),
                    Span::styled("  ", Style::default()),
                    Span::styled(p.api_backend.as_str(), Style::default().fg(theme.gray)),
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        key_status,
                        Style::default().fg(if p.has_key {
                            theme.accent_success
                        } else {
                            theme.accent_error
                        }),
                    ),
                ],
            );
            y += 1;
        }
        y += 1;
    }

    // 「+ 添加渠道」固定末行
    if y < area.y + area.height {
        let add_idx = state.providers.len();
        let selected = state.selected == add_idx;
        paint_list_row(
            buf,
            area,
            y,
            selected,
            theme,
            vec![
                Span::styled(
                    "  + 添加渠道",
                    Style::default().fg(theme.text_primary),
                ),
                Span::styled("  (a)", Style::default().fg(theme.gray_dim)),
            ],
        );
        y += 1;
    }

    if let Some(err) = &state.error {
        if y < area.y + area.height {
            let err_line = Line::from(Span::styled(
                format!("✗ {err}"),
                Style::default().fg(theme.accent_error),
            ));
            err_line.render(Rect::new(area.x, y, area.width, 1), buf);
        }
    }
}

// ── 二级操作菜单 ───────────────────────────────────────────────────────────

fn render_actions(
    buf: &mut Buffer,
    area: Rect,
    state: &mut ProviderModalState,
    name: &str,
    theme: &Theme,
    compact: bool,
) {
    let sizing = provider_sizing(compact);
    let shortcuts: &[mw::Shortcut<'static>] = &[
        mw::Shortcut {
            label: "Enter 确认",
            clickable: false,
            id: 0,
        },
        mw::Shortcut {
            label: "Esc 返回",
            clickable: false,
            id: 1,
        },
    ];
    let title = format!("渠道 · {name}");
    let config = ModalWindowConfig {
        title: &title,
        tabs: None,
        shortcuts,
        sizing,
        fold_info: None,
    };

    let Some(mca) = mw::render_modal_window(buf, area, &mut state.window, &config, theme) else {
        return;
    };
    let content = mca.content;

    let mut y = content.y;
    let hint = Line::from(Span::styled(
        "选择操作:",
        Style::default().fg(theme.gray_dim),
    ));
    hint.render(Rect::new(content.x, y, content.width, 1), buf);
    y += 1;

    for (i, action) in ProviderAction::ALL.iter().enumerate() {
        if y >= content.y + content.height {
            break;
        }
        let selected = i == state.selected;
        paint_list_row(
            buf,
            content,
            y,
            selected,
            theme,
            vec![
                Span::styled(
                    format!("  {}", action.label()),
                    Style::default().fg(theme.text_primary),
                ),
                Span::styled(
                    format!("  {}", action.hint()),
                    Style::default().fg(theme.gray_dim),
                ),
            ],
        );
        y += 1;
    }

    if let Some(succ) = &state.success {
        if y + 1 < content.y + content.height {
            y += 1;
            let succ_line = Line::from(Span::styled(
                format!("✓ {succ}"),
                Style::default().fg(theme.accent_success),
            ));
            succ_line.render(Rect::new(content.x, y, content.width, 1), buf);
        }
    }
}

// ── /provider models <name> ────────────────────────────────────────────────

fn render_models(
    buf: &mut Buffer,
    area: Rect,
    state: &mut ProviderModalState,
    name: &str,
    theme: &Theme,
    compact: bool,
) {
    let sizing = provider_sizing(compact);
    let shortcuts: &[mw::Shortcut<'static>] = if state.from_hub {
        &[
            mw::Shortcut {
                label: "输入筛选",
                clickable: false,
                id: 0,
            },
            mw::Shortcut {
                label: "↑↓ 滚动",
                clickable: false,
                id: 1,
            },
            mw::Shortcut {
                label: "Enter 切换",
                clickable: false,
                id: 2,
            },
            mw::Shortcut {
                label: "Esc 返回",
                clickable: false,
                id: 3,
            },
        ]
    } else {
        &[
            mw::Shortcut {
                label: "输入筛选",
                clickable: false,
                id: 0,
            },
            mw::Shortcut {
                label: "↑↓ 滚动",
                clickable: false,
                id: 1,
            },
            mw::Shortcut {
                label: "Esc 关闭",
                clickable: false,
                id: 2,
            },
        ]
    };
    let title = format!("渠道 \"{name}\" 可用模型");
    let config = ModalWindowConfig {
        title: &title,
        tabs: None,
        shortcuts,
        sizing,
        fold_info: None,
    };

    let Some(mca) = mw::render_modal_window(buf, area, &mut state.window, &config, theme) else {
        return;
    };
    render_model_list_body(buf, mca.content, state, theme);
}

// ── /provider set-model <name> ─────────────────────────────────────────────

fn render_set_model(
    buf: &mut Buffer,
    area: Rect,
    state: &mut ProviderModalState,
    name: &str,
    theme: &Theme,
    compact: bool,
) {
    let sizing = provider_sizing(compact);
    let back = if state.from_hub {
        "Esc 返回"
    } else {
        "Esc 取消"
    };
    let shortcuts: &[mw::Shortcut<'static>] = &[
        mw::Shortcut {
            label: "输入筛选",
            clickable: false,
            id: 0,
        },
        mw::Shortcut {
            label: "↑↓ 滚动",
            clickable: false,
            id: 1,
        },
        mw::Shortcut {
            label: "Enter 确认",
            clickable: false,
            id: 2,
        },
        mw::Shortcut {
            label: back,
            clickable: false,
            id: 3,
        },
    ];
    let title = format!("为渠道 \"{name}\" 选择模型");
    let config = ModalWindowConfig {
        title: &title,
        tabs: None,
        shortcuts,
        sizing,
        fold_info: None,
    };

    let Some(mca) = mw::render_modal_window(buf, area, &mut state.window, &config, theme) else {
        return;
    };
    render_model_list_body(buf, mca.content, state, theme);
}

/// Shared body for Models / SetModel: search bar + filtered scrolled list.
fn render_model_list_body(
    buf: &mut Buffer,
    content: Rect,
    state: &mut ProviderModalState,
    theme: &Theme,
) {
    if state.models_loading {
        let line = Line::from(Span::styled(
            "正在获取模型列表…",
            Style::default().fg(theme.gray),
        ));
        line.render(content, buf);
        return;
    }

    if let Some(err) = &state.error {
        let line = Line::from(Span::styled(
            format!("✗ {err}"),
            Style::default().fg(theme.accent_error),
        ));
        line.render(content, buf);
        return;
    }

    if state.models.is_empty() {
        let line = Line::from(Span::styled(
            "未返回任何模型。",
            Style::default().fg(theme.gray),
        ));
        line.render(content, buf);
        return;
    }

    // Layout: [search bar] [divider] [list…] [optional status]
    let mut y = content.y;
    let bottom = content.y + content.height;

    // ── search bar ──
    if y < bottom {
        render_model_search_bar(buf, content.x, y, content.width, state, theme);
        y += 1;
    }
    // divider under search (matches settings / picker chrome)
    if y < bottom {
        let div: String = std::iter::repeat_n('─', content.width as usize).collect();
        buf.set_string(content.x, y, &div, Style::default().fg(theme.gray_dim));
        y += 1;
    }

    let filtered_len = state.filtered_model_count();
    let catalog_total = state.models.len();

    if filtered_len == 0 {
        if y < bottom {
            let line = Line::from(Span::styled(
                "  无匹配模型（Esc 清除筛选）",
                Style::default().fg(theme.gray),
            ));
            line.render(Rect::new(content.x, y, content.width, 1), buf);
        }
        state.set_list_viewport(0, 0);
        return;
    }

    let remaining = bottom.saturating_sub(y) as usize;
    // Status when filtered list is long, or when filter is active (show match count).
    let filter_active = !state.model_filter.trim().is_empty();
    let need_status = filtered_len > remaining.saturating_sub(1).max(1) || filter_active;
    let list_h = if need_status {
        remaining.saturating_sub(1).max(1)
    } else {
        remaining.max(1)
    };
    // Mutate scroll/viewport before borrowing filtered ids for paint.
    state.set_list_viewport(list_h, filtered_len);

    let start = state.scroll_offset.min(filtered_len.saturating_sub(1));
    let end = (start + list_h).min(filtered_len);
    let filtered = state.filtered_models();

    for i in start..end {
        if y >= bottom {
            break;
        }
        let m = filtered[i];
        let selected = i == state.selected;
        paint_list_row(
            buf,
            content,
            y,
            selected,
            theme,
            vec![Span::styled(
                format!("  {m}"),
                Style::default().fg(theme.text_primary),
            )],
        );
        y += 1;
    }

    if need_status && y < bottom {
        let status = if filter_active {
            format!(
                "  {}–{} / {} 匹配 · 共 {}  ·  ↑↓  PgUp/Dn  Esc 清筛选",
                if filtered_len == 0 { 0 } else { start + 1 },
                end,
                filtered_len,
                catalog_total
            )
        } else {
            format!(
                "  {}–{} / {}  ·  输入筛选  ↑↓  PgUp/Dn  Home/End",
                start + 1,
                end,
                catalog_total
            )
        };
        let line = Line::from(Span::styled(
            status,
            Style::default().fg(theme.gray_dim),
        ));
        line.render(Rect::new(content.x, y, content.width, 1), buf);
    }
}

/// Top search field: ` 搜索: <query>█` with placeholder when empty.
fn render_model_search_bar(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    state: &ProviderModalState,
    theme: &Theme,
) {
    let label = " 搜索: ";
    let label_w = label.width() as u16;
    let label_style = Style::default()
        .fg(theme.accent_user)
        .add_modifier(Modifier::BOLD);
    let query_style = Style::default().fg(theme.text_primary);
    let placeholder_style = Style::default().fg(theme.gray_dim);

    // Paint row background (search is always the focused text field).
    let row = Rect {
        x,
        y,
        width,
        height: 1,
    };
    let bg = list_row_bg(theme, false);
    buf.set_style(row, Style::default().bg(bg));

    buf.set_string(x, y, label, label_style.bg(bg));

    let input_x = x.saturating_add(label_w);
    let input_w = width.saturating_sub(label_w);
    if input_w == 0 {
        return;
    }

    let query = state.model_filter.as_str();
    if query.is_empty() {
        let hint = "输入模型名筛选…";
        let hint_disp = truncate_str(hint, input_w as usize);
        buf.set_string(input_x, y, &hint_disp, placeholder_style.bg(bg));
        // Cursor at start of placeholder
        if let Some(cell) = buf.cell_mut((input_x, y)) {
            cell.set_style(Style::default().fg(theme.bg_dark).bg(theme.text_primary));
        }
    } else {
        let disp = truncate_str(query, input_w.saturating_sub(1) as usize);
        buf.set_string(input_x, y, &disp, query_style.bg(bg));
        let cursor_x = input_x + disp.width() as u16;
        if cursor_x < x + width {
            if let Some(cell) = buf.cell_mut((cursor_x, y)) {
                cell.set_style(Style::default().fg(theme.bg_dark).bg(theme.text_primary));
            }
        }
    }
}

// ── 工具函数 ────────────────────────────────────────────────────────────────

/// 用 • 遮蔽 API Key 内容（保留后 4 位用于确认）。
fn mask_key(key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    if key.len() <= 4 {
        return "•".repeat(key.len());
    }
    let tail = &key[key.len() - 4..];
    format!("{}{}", "•".repeat(key.len() - 4), tail)
}

/// 截断字符串到指定显示宽度。
fn truncate_str(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }
    let mut result = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw > max_width.saturating_sub(1) {
            result.push('…');
            break;
        }
        result.push(c);
        w += cw;
    }
    result
}
