//! Provider modal rendering.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::Theme;
use crate::views::modal_window::{self as mw, ModalWindowConfig, ModalSizing};

use super::state::{
    API_BACKENDS, AUTH_SCHEMES, FormStep, ProviderAction, ProviderModalMode, ProviderModalState,
    PROVIDER_PRESETS,
};

/// 渲染 Provider 模态框。
pub fn render_provider_modal(buf: &mut Buffer, area: Rect, state: &mut ProviderModalState) {
    let theme = Theme::current();
    let mode = state.mode.clone();
    match &mode {
        ProviderModalMode::SetKey(name) => render_set_key(buf, area, state, name, &theme),
        ProviderModalMode::Models(name) => render_models(buf, area, state, name, &theme),
        ProviderModalMode::SetModel(name) => render_set_model(buf, area, state, name, &theme),
        ProviderModalMode::Actions(name) => render_actions(buf, area, state, name, &theme),
        ProviderModalMode::List | ProviderModalMode::Add => {
            let title = match &mode {
                ProviderModalMode::Add => "添加渠道",
                _ => "渠道管理",
            };
            let sizing = ModalSizing {
                width_pct: 0.55,
                max_width: 90,
                min_width: 50,
                v_margin: 6,
                h_pad: 2,
                v_pad: 1,
                footer_lines: 2,
            };
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
    let selected_style = Style::default()
        .fg(theme.accent_user)
        .add_modifier(Modifier::BOLD);

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
            let style = if i == state.selected {
                selected_style
            } else {
                value_style
            };
            let prefix = if i == state.selected { "▸ " } else { "  " };
            let line = Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(p.display, style),
                Span::styled(format!("  ({})", p.base_url), dim_style),
            ]);
            line.render(Rect::new(area.x, y, area.width, 1), buf);
            y += 1;
        }
        // 自定义选项
        let custom_idx = PROVIDER_PRESETS.len();
        if y < area.y + area.height {
            let style = if state.selected == custom_idx {
                selected_style
            } else {
                value_style
            };
            let prefix = if state.selected == custom_idx {
                "▸ "
            } else {
                "  "
            };
            let line = Line::from(vec![
                Span::styled(prefix, style),
                Span::styled("自定义", style),
            ]);
            line.render(Rect::new(area.x, y, area.width, 1), buf);
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

// ── /provider set-key <name> ───────────────────────────────────────────────

fn render_set_key(
    buf: &mut Buffer,
    area: Rect,
    state: &mut ProviderModalState,
    name: &str,
    theme: &Theme,
) {
    let sizing = ModalSizing {
        width_pct: 0.50,
        max_width: 80,
        min_width: 50,
        v_margin: 8,
        h_pad: 2,
        v_pad: 1,
        footer_lines: 2,
    };
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
    let normal_style = Style::default().fg(theme.text_primary);
    let selected_style = Style::default()
        .fg(theme.accent_user)
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
        // 表头
        let header = Line::from(vec![
            Span::styled("名称", header_style),
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

        // 分隔线
        if y < area.y + area.height {
            let div: String = std::iter::repeat_n('─', area.width as usize).collect();
            buf.set_string(area.x, y, &div, dim_style);
            y += 1;
        }

        for (i, p) in state.providers.iter().enumerate() {
            if y >= area.y + area.height {
                break;
            }
            let style = if i == state.selected {
                selected_style
            } else {
                normal_style
            };
            let marker = if p.is_current { " *" } else { "  " };
            let key_status = if p.has_key { "✓" } else { "✗" };
            let prefix = if i == state.selected { "▸ " } else { "  " };
            let row = Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(format!("{}{}", p.name, marker), style),
                Span::styled("  ", style),
                Span::styled(truncate_str(&p.base_url, 28), style),
                Span::styled("  ", style),
                Span::styled(&p.auth_scheme, style),
                Span::styled("  ", style),
                Span::styled(&p.api_backend, style),
                Span::styled("  ", style),
                Span::styled(key_status, style),
            ]);
            row.render(Rect::new(area.x, y, area.width, 1), buf);
            y += 1;
        }
        y += 1;
    }

    // 「+ 添加渠道」固定末行
    if y < area.y + area.height {
        let add_idx = state.providers.len();
        let style = if state.selected == add_idx {
            selected_style
        } else {
            normal_style
        };
        let prefix = if state.selected == add_idx {
            "▸ "
        } else {
            "  "
        };
        let line = Line::from(vec![
            Span::styled(prefix, style),
            Span::styled("+ 添加渠道", style),
            Span::styled("  (a)", dim_style),
        ]);
        line.render(Rect::new(area.x, y, area.width, 1), buf);
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
) {
    let sizing = ModalSizing {
        width_pct: 0.45,
        max_width: 70,
        min_width: 40,
        v_margin: 8,
        h_pad: 2,
        v_pad: 1,
        footer_lines: 2,
    };
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

    let normal_style = Style::default().fg(theme.text_primary);
    let selected_style = Style::default()
        .fg(theme.accent_user)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(theme.gray_dim);

    let mut y = content.y;
    let hint = Line::from(Span::styled("选择操作:", dim_style));
    hint.render(Rect::new(content.x, y, content.width, 1), buf);
    y += 1;

    for (i, action) in ProviderAction::ALL.iter().enumerate() {
        if y >= content.y + content.height {
            break;
        }
        let style = if i == state.selected {
            selected_style
        } else {
            normal_style
        };
        let prefix = if i == state.selected { "▸ " } else { "  " };
        let line = Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(action.label(), style),
            Span::styled(format!("  {}", action.hint()), dim_style),
        ]);
        line.render(Rect::new(content.x, y, content.width, 1), buf);
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
) {
    let sizing = ModalSizing {
        width_pct: 0.60,
        max_width: 100,
        min_width: 50,
        v_margin: 5,
        h_pad: 2,
        v_pad: 1,
        footer_lines: 2,
    };
    let shortcuts: &[mw::Shortcut<'static>] = if state.from_hub {
        &[
            mw::Shortcut {
                label: "Enter 切换",
                clickable: false,
                id: 0,
            },
            mw::Shortcut {
                label: "Esc 返回",
                clickable: false,
                id: 1,
            },
        ]
    } else {
        &[mw::Shortcut {
            label: "Esc 关闭",
            clickable: false,
            id: 0,
        }]
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
    let content = mca.content;

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

    let normal_style = Style::default().fg(theme.text_primary);
    let selected_style = Style::default()
        .fg(theme.accent_user)
        .add_modifier(Modifier::BOLD);

    let mut y = content.y;
    for (i, m) in state.models.iter().enumerate() {
        if y >= content.y + content.height {
            break;
        }
        let style = if i == state.selected {
            selected_style
        } else {
            normal_style
        };
        let prefix = if i == state.selected { "▸ " } else { "  " };
        let line = Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(m, style),
        ]);
        line.render(Rect::new(content.x, y, content.width, 1), buf);
        y += 1;
    }
}

// ── /provider set-model <name> ─────────────────────────────────────────────

fn render_set_model(
    buf: &mut Buffer,
    area: Rect,
    state: &mut ProviderModalState,
    name: &str,
    theme: &Theme,
) {
    let sizing = ModalSizing {
        width_pct: 0.60,
        max_width: 100,
        min_width: 50,
        v_margin: 5,
        h_pad: 2,
        v_pad: 1,
        footer_lines: 2,
    };
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
    let content = mca.content;

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

    let normal_style = Style::default().fg(theme.text_primary);
    let selected_style = Style::default()
        .fg(theme.accent_user)
        .add_modifier(Modifier::BOLD);

    let mut y = content.y;
    for (i, m) in state.models.iter().enumerate() {
        if y >= content.y + content.height {
            break;
        }
        let style = if i == state.selected {
            selected_style
        } else {
            normal_style
        };
        let prefix = if i == state.selected { "▸ " } else { "  " };
        let line = Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(m, style),
        ]);
        line.render(Rect::new(content.x, y, content.width, 1), buf);
        y += 1;
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
