//! In-TUI `/doctor` formatter tests.

use super::format_doctor;
use crate::clipboard::ClipboardRoute;
use crate::diagnostics::probes::{
    ClipboardProbeFacts, DoctorProbeSnapshot, ProbeSnapshot, TmuxProbeFacts, TmuxProbeResult,
    TuiProbeEvidence, WaylandProbeFacts,
};
use crate::diagnostics::{
    ClipboardFacts, ColorFacts, DataControlFact, DiagnosticFacts, DiagnosticReport,
    DiagnosticSnapshot, KeyboardFact, RuntimeFact, view,
};
use crate::host::HostOs;
use crate::terminal::{
    ByobuBackend, ModifierDelivery, ModifierFate, MultiplexerKind, TerminalContext, TerminalName,
};
use crate::theme::color_support::ColorLevel;

static LOCAL_ROUTE: ClipboardRoute = ClipboardRoute {
    native: true,
    tmux_buffer: false,
    osc52: false,
    osc52_tmux_passthrough: false,
};
static SSH_ROUTE: ClipboardRoute = ClipboardRoute {
    native: true,
    tmux_buffer: false,
    osc52: true,
    osc52_tmux_passthrough: false,
};
static TMUX_ROUTE: ClipboardRoute = ClipboardRoute {
    native: true,
    tmux_buffer: true,
    osc52: true,
    osc52_tmux_passthrough: true,
};

fn unavailable_tmux() -> TmuxProbeFacts {
    TmuxProbeFacts {
        version: TmuxProbeResult::Unavailable,
        extended_keys: TmuxProbeResult::Unavailable,
        set_clipboard: TmuxProbeResult::Unavailable,
        allow_passthrough_support: TmuxProbeResult::Unavailable,
        allow_passthrough: TmuxProbeResult::Unavailable,
        control_mode: TmuxProbeResult::Unavailable,
    }
}

fn snapshot<'a>(
    terminal: &'a TerminalContext,
    tmux: TmuxProbeFacts,
    route: &'static ClipboardRoute,
    native_tool: &'static str,
    osc52_sink_active: bool,
    color_level: ColorLevel,
    runtime: TuiProbeEvidence<'a>,
) -> DoctorProbeSnapshot<'a> {
    DoctorProbeSnapshot {
        common: ProbeSnapshot {
            terminal,
            tmux,
            wayland: WaylandProbeFacts {
                is_wayland: false,
                data_control: TmuxProbeResult::Available(false),
                wl_copy_available: false,
            },
            runtime,
        },
        clipboard: ClipboardProbeFacts {
            route: route.clone(),
            native_tool,
            osc52_sink_active,
        },
        host_os: HostOs::Macos,
        display_server: crate::host::DisplayServer::Unknown,
        container_no_display: false,
        color_level,
    }
}

fn runtime<'a>(xtversion: Option<&'a str>, kitty_flags_pushed: bool) -> TuiProbeEvidence<'a> {
    TuiProbeEvidence {
        fullscreen_active: true,
        kitty_flags_pushed,
        xtversion,
    }
}

fn ghostty(is_ssh: bool) -> TerminalContext {
    TerminalContext {
        brand: TerminalName::Ghostty,
        env_brand: TerminalName::Ghostty,
        is_ssh,
        ..Default::default()
    }
}

fn build_doctor(snapshot: DoctorProbeSnapshot<'_>) -> String {
    let report = view(DiagnosticSnapshot::from(snapshot));
    format_doctor(&report)
}

fn build_doctor_with_runtime(
    snapshot: DoctorProbeSnapshot<'_>,
    request: crate::diagnostics::TuiRuntimeRequest<'_>,
) -> String {
    let findings = crate::diagnostics::collect_tui_runtime_findings(
        &snapshot.common,
        request.notification_method,
        request.notification_protocol,
        request.notification_condition,
        request.workspace,
    );
    let mut report = view(snapshot.into());
    crate::diagnostics::merge_tui_runtime_findings(&mut report, findings);
    format_doctor(&report)
}

#[test]
fn healthy_local_output_is_stable() {
    let terminal = ghostty(false);
    let output = build_doctor(snapshot(
        &terminal,
        unavailable_tmux(),
        &LOCAL_ROUTE,
        "pbcopy",
        false,
        ColorLevel::TrueColor,
        runtime(None, true),
    ));

    assert_eq!(
        output,
        concat!(
            "环境\n",
            "  terminal     Ghostty\n",
            "  multiplexer  None detected\n",
            "  ssh          no\n",
            "  color        truecolor\n",
            "  themes       all\n",
            "\n",
            "剪贴板\n",
            "  native       local (pbcopy)\n",
            "  tmux         off\n",
            "  osc 52       off\n",
            "  wrap         off\n",
            "  status       confirmed\n",
            "\n",
            "未发现问题。\n",
        )
    );
}

#[test]
fn tmux_config_and_reload_notes_output_is_stable() {
    let terminal = TerminalContext {
        brand: TerminalName::Iterm2,
        env_brand: TerminalName::Iterm2,
        multiplexer: MultiplexerKind::Tmux,
        byobu: Some(ByobuBackend::Tmux),
        tmux_version: Some("tmux 3.4".to_owned()),
        tmux_extended_keys: Some("off".to_owned()),
        ..Default::default()
    };
    let output = build_doctor(snapshot(
        &terminal,
        TmuxProbeFacts {
            version: TmuxProbeResult::Unavailable,
            extended_keys: TmuxProbeResult::Available("off".to_owned()),
            set_clipboard: TmuxProbeResult::Available("off".to_owned()),
            allow_passthrough_support: TmuxProbeResult::Available(()),
            allow_passthrough: TmuxProbeResult::Available("off".to_owned()),
            control_mode: TmuxProbeResult::Available(false),
        },
        &TMUX_ROUTE,
        "pbcopy",
        false,
        ColorLevel::TrueColor,
        runtime(None, false),
    ));

    assert_eq!(
        output,
        concat!(
            "环境\n",
            "  terminal     iTerm2\n",
            "  multiplexer  tmux\n",
            "  byobu        tmux\n",
            "  ssh          no\n",
            "  color        truecolor\n",
            "  themes       all\n",
            "\n",
            "剪贴板\n",
            "  native       local (pbcopy)\n",
            "  tmux         on\n",
            "  osc 52       supported\n",
            "  wrap         off\n",
            "  status       confirmed\n",
            "\n",
            "问题 (3)\n",
            "\n",
            "  ! terminal.tmux-clipboard  tmux 中 `set-clipboard` 已关闭，OSC 52 剪贴板复制被阻止\n",
            "      自动修复：`chaos doctor fix tmux-clipboard`\n",
            "      在 ~/.byobu/.tmux.conf 中添加 `set -g set-clipboard on`\n",
            "      说明：请用 `tmux source-file ~/.byobu/.tmux.conf` 重载 tmux，或先 detach 再 reattach。\n",
            "\n",
            "  ! terminal.dcs-passthrough  tmux 中 `allow-passthrough` 已关闭，嵌套会话中的剪贴板复制可能被阻止\n",
            "      自动修复：`chaos doctor fix dcs-passthrough`\n",
            "      在 ~/.byobu/.tmux.conf 中添加 `set -wg allow-passthrough on`\n",
            "      说明：请用 `tmux source-file ~/.byobu/.tmux.conf` 重载 tmux，或先 detach 再 reattach。\n",
            "\n",
            "  ! terminal.tmux-extended-keys  tmux 中 `extended-keys` 已关闭，部分快捷键可能无效\n",
            "      自动修复：`chaos doctor fix tmux-extended-keys`\n",
            "      在 ~/.byobu/.tmux.conf 中添加 `set -g extended-keys on`\n",
            "      说明：请用 `tmux source-file ~/.byobu/.tmux.conf` 重载 tmux，或先 detach 再 reattach。\n",
        )
    );
}

#[test]
fn limited_color_output_is_stable() {
    let terminal = ghostty(false);
    let output = build_doctor(snapshot(
        &terminal,
        unavailable_tmux(),
        &LOCAL_ROUTE,
        "pbcopy",
        false,
        ColorLevel::Ansi256,
        runtime(None, true),
    ));

    assert_eq!(
        output,
        concat!(
            "环境\n",
            "  terminal     Ghostty\n",
            "  multiplexer  None detected\n",
            "  ssh          no\n",
            "  color        256\n",
            "  themes       2/5: groknight, grokday\n",
            "\n",
            "剪贴板\n",
            "  native       local (pbcopy)\n",
            "  tmux         off\n",
            "  osc 52       off\n",
            "  wrap         off\n",
            "  status       confirmed\n",
            "\n",
            "问题 (1)\n",
            "\n",
            "  ! terminal.limited-color  此终端报告为 256 色，因此 truecolor 主题不可用\n",
            "      运行：`export COLORTERM=truecolor`\n",
            "      说明：请将此 export 写入 shell 启动文件（如 `~/.zshrc` 或 `~/.bashrc`），然后重启 Chaos。\n",
        )
    );
}

#[test]
fn unwrapped_ssh_recommendation_with_no_issues_output_is_stable() {
    let terminal = ghostty(true);
    let output = build_doctor(snapshot(
        &terminal,
        unavailable_tmux(),
        &SSH_ROUTE,
        "pbcopy",
        false,
        ColorLevel::TrueColor,
        runtime(None, true),
    ));

    assert_eq!(
        output,
        concat!(
            "环境\n",
            "  terminal     Ghostty\n",
            "  multiplexer  None detected\n",
            "  ssh          yes\n",
            "  color        truecolor\n",
            "  themes       all\n",
            "\n",
            "剪贴板\n",
            "  native       remote (pbcopy)\n",
            "  tmux         off\n",
            "  osc 52       supported\n",
            "  wrap         off\n",
            "  status       confirmed\n",
            "\n",
            "未发现问题。\n",
            "\n",
            "建议\n",
            "\n",
            "  i terminal.ssh-wrap  建议在本地使用 SSH 包装，以获得更可靠的剪贴板复制与终端恢复\n",
            "      自动修复：`chaos doctor fix ssh-wrap`\n",
            "      一次性：`chaos wrap ssh <host>`\n",
            "      说明：请在本地电脑运行，而不是直接使用普通 `ssh`。它会把复制转发到本地剪贴板，并在连接断开时恢复终端模式。\n",
        )
    );
}

#[test]
fn wrapped_ssh_output_has_no_recommendation() {
    let terminal = ghostty(true);
    let output = build_doctor(snapshot(
        &terminal,
        unavailable_tmux(),
        &SSH_ROUTE,
        "pbcopy",
        true,
        ColorLevel::TrueColor,
        runtime(None, true),
    ));

    assert_eq!(
        output,
        concat!(
            "环境\n",
            "  terminal     Ghostty\n",
            "  multiplexer  None detected\n",
            "  ssh          yes\n",
            "  color        truecolor\n",
            "  themes       all\n",
            "\n",
            "剪贴板\n",
            "  native       remote (pbcopy)\n",
            "  tmux         off\n",
            "  osc 52       supported\n",
            "  wrap         on\n",
            "  status       confirmed\n",
            "\n",
            "未发现问题。\n",
        )
    );
}

#[test]
fn wezterm_xtversion_runtime_evidence_output_is_stable() {
    let terminal = TerminalContext {
        is_ssh: true,
        ..Default::default()
    };
    let output = build_doctor(snapshot(
        &terminal,
        unavailable_tmux(),
        &SSH_ROUTE,
        "pbcopy",
        true,
        ColorLevel::TrueColor,
        runtime(Some("WezTerm 20240203-110809"), false),
    ));

    assert_eq!(
        output,
        concat!(
            "环境\n",
            "  terminal     Unknown\n",
            "  xtversion    WezTerm 20240203-110809\n",
            "  multiplexer  None detected\n",
            "  ssh          yes\n",
            "  color        truecolor\n",
            "  themes       all\n",
            "\n",
            "剪贴板\n",
            "  native       remote (pbcopy)\n",
            "  tmux         off\n",
            "  osc 52       supported\n",
            "  wrap         on\n",
            "  status       confirmed\n",
            "\n",
            "问题 (1)\n",
            "\n",
            "  ! terminal.wezterm-kitty  在 SSH 下的 WezTerm 中，Shift+Enter 无法插入换行\n",
            "      说明：本次会话请先输入 `\\` 再按 Enter。Chaos 尚无法在 SSH 上协商 Kitty 键盘协议。`enable_kitty_keyboard = true` 仅对本地 WezTerm 会话生效。\n",
        )
    );
}

#[test]
fn unavailable_and_error_probes_do_not_create_false_issues() {
    let terminal = TerminalContext {
        brand: TerminalName::Iterm2,
        env_brand: TerminalName::Iterm2,
        multiplexer: MultiplexerKind::Tmux,
        tmux_version: Some("tmux 3.4".to_owned()),
        ..Default::default()
    };
    let output = build_doctor(snapshot(
        &terminal,
        TmuxProbeFacts {
            version: TmuxProbeResult::Unavailable,
            extended_keys: TmuxProbeResult::Unavailable,
            set_clipboard: TmuxProbeResult::Error("tmux server unreachable".to_owned()),
            allow_passthrough_support: TmuxProbeResult::Unavailable,
            allow_passthrough: TmuxProbeResult::Error("query failed".to_owned()),
            control_mode: TmuxProbeResult::Unavailable,
        },
        &TMUX_ROUTE,
        "pbcopy",
        false,
        ColorLevel::TrueColor,
        runtime(None, false),
    ));

    assert_eq!(
        output,
        concat!(
            "环境\n",
            "  terminal     iTerm2\n",
            "  multiplexer  tmux\n",
            "  ssh          no\n",
            "  color        truecolor\n",
            "  themes       all\n",
            "\n",
            "剪贴板\n",
            "  native       local (pbcopy)\n",
            "  tmux         on\n",
            "  osc 52       supported\n",
            "  wrap         off\n",
            "  status       confirmed\n",
            "\n",
            "未发现问题。\n",
        )
    );
}

#[test]
fn vscode_newline_output_is_platform_neutral() {
    let terminal = TerminalContext {
        brand: TerminalName::VsCode,
        env_brand: TerminalName::VsCode,
        ..Default::default()
    };
    let output = build_doctor(snapshot(
        &terminal,
        unavailable_tmux(),
        &LOCAL_ROUTE,
        "pbcopy",
        false,
        ColorLevel::TrueColor,
        runtime(None, false),
    ));

    assert_eq!(
        output,
        concat!(
            "环境\n",
            "  terminal     VS Code\n",
            "  multiplexer  None detected\n",
            "  ssh          no\n",
            "  color        truecolor\n",
            "  themes       all\n",
            "  newline      Alt+Enter（VS Code：xterm.js 无法区分 Shift+Enter）\n",
            "\n",
            "剪贴板\n",
            "  native       local (pbcopy)\n",
            "  tmux         off\n",
            "  osc 52       off\n",
            "  wrap         off\n",
            "  status       confirmed\n",
            "\n",
            "未发现问题。\n",
            "\n",
            "建议\n",
            "\n",
            "  i terminal.newline-fallback  在此 xterm.js 终端中，Shift+Enter 无法插入换行\n",
            "      说明：在 VS Code 中请使用 Alt+Enter 插入换行。在此环境下 xterm.js 会把 Shift+Enter 当作 Enter 发送。\n",
        )
    );
}

#[test]
fn runtime_merge_does_not_duplicate_view_findings() {
    let terminal = TerminalContext {
        brand: TerminalName::Iterm2,
        env_brand: TerminalName::Iterm2,
        multiplexer: MultiplexerKind::Tmux,
        tmux_extended_keys: Some("off".to_owned()),
        ..Default::default()
    };
    let workspace = tempfile::tempdir().unwrap();
    let output = build_doctor_with_runtime(
        snapshot(
            &terminal,
            TmuxProbeFacts {
                version: TmuxProbeResult::Available("tmux 3.4".to_owned()),
                extended_keys: TmuxProbeResult::Available("off".to_owned()),
                set_clipboard: TmuxProbeResult::Available("off".to_owned()),
                allow_passthrough_support: TmuxProbeResult::Available(()),
                allow_passthrough: TmuxProbeResult::Available("off".to_owned()),
                control_mode: TmuxProbeResult::Available(false),
            },
            &TMUX_ROUTE,
            "pbcopy",
            false,
            ColorLevel::Ansi256,
            runtime(None, false),
        ),
        crate::diagnostics::TuiRuntimeRequest {
            workspace: workspace.path(),
            notification_method: crate::notifications::NotificationMethod::Auto,
            notification_protocol: crate::notifications::protocol::NotificationProtocol::Bel,
            notification_condition: crate::notifications::NotificationCondition::Always,
        },
    );

    for id in [
        "terminal.tmux-clipboard",
        "terminal.dcs-passthrough",
        "terminal.tmux-extended-keys",
        "terminal.limited-color",
    ] {
        assert_eq!(output.matches(id).count(), 1, "{id}:\n{output}");
    }
    assert!(output.contains("问题 (4)"), "{output}");
}

#[test]
fn runtime_startup_findings_are_visible_with_useful_doctor_content() {
    let terminal = TerminalContext::default();
    let workspace = tempfile::tempdir().unwrap();
    let output = build_doctor_with_runtime(
        snapshot(
            &terminal,
            unavailable_tmux(),
            &LOCAL_ROUTE,
            "pbcopy",
            false,
            ColorLevel::TrueColor,
            runtime(None, true),
        ),
        crate::diagnostics::TuiRuntimeRequest {
            workspace: workspace.path(),
            notification_method: crate::notifications::NotificationMethod::Auto,
            notification_protocol: crate::notifications::protocol::NotificationProtocol::Bel,
            notification_condition: crate::notifications::NotificationCondition::Unfocused,
        },
    );

    assert!(output.contains("因未识别终端，Chaos 正使用终端响铃作为通知"));
    assert!(output.contains("若响铃可用"));
    assert!(output.contains("此终端可能不报告焦点变化"));
    assert!(output.contains(&crate::util::display_user_grok_path("config.toml")));
    assert_eq!(output.matches("notifications.protocol-fallback").count(), 1);
    assert_eq!(
        output
            .matches("notifications.focus-tracking-unavailable")
            .count(),
        1
    );
    assert!(!output.contains("未发现问题。"));
    assert!(output.contains("问题 (2)"));
    assert!(output.contains("terminal.newline-fallback"));
    assert!(output.contains("建议"));
}

#[test]
fn runtime_findings_merge_before_single_formatter_orders_issues_before_recommendations() {
    let terminal = TerminalContext {
        brand: TerminalName::Unknown,
        env_brand: TerminalName::Unknown,
        is_ssh: true,
        ..Default::default()
    };
    let workspace = tempfile::tempdir().unwrap();
    let output = build_doctor_with_runtime(
        snapshot(
            &terminal,
            unavailable_tmux(),
            &SSH_ROUTE,
            "pbcopy",
            false,
            ColorLevel::TrueColor,
            runtime(None, true),
        ),
        crate::diagnostics::TuiRuntimeRequest {
            workspace: workspace.path(),
            notification_method: crate::notifications::NotificationMethod::Auto,
            notification_protocol: crate::notifications::protocol::NotificationProtocol::Bel,
            notification_condition: crate::notifications::NotificationCondition::Unfocused,
        },
    );

    let issue = output.find("因未识别终端，Chaos 正使用终端响铃作为通知").unwrap();
    let recommendation = output.find("建议").unwrap();
    assert!(issue < recommendation);
    assert!(!output.contains("未发现问题。"));
    assert_eq!(output.matches("问题 (").count(), 1);
}

#[test]
fn legacy_fact_only_clipboard_issue_never_claims_no_issues() {
    let terminal = ghostty(false);
    let mut report = view(DiagnosticSnapshot::from(snapshot(
        &terminal,
        unavailable_tmux(),
        &LOCAL_ROUTE,
        "pbcopy",
        false,
        ColorLevel::TrueColor,
        runtime(None, true),
    )));
    report.facts.clipboard.delivery = crate::clipboard::ClipboardDelivery::Failed;
    assert_eq!(report.issue_count(), 1);
    let output = format_doctor(&report);
    assert!(output.contains("问题已显示在上方剪贴板状态中。"));
    assert!(!output.contains("未发现问题。"));
}

#[test]
fn keyboard_fact_formats_from_explicit_target_evidence() {
    let report = DiagnosticReport {
        facts: DiagnosticFacts {
            terminal: TerminalName::WezTerm,
            xtversion: RuntimeFact::NoReply,
            multiplexer: MultiplexerKind::Undetected,
            byobu: None,
            ssh: false,
            tmux: crate::diagnostics::TmuxFacts {
                extended_keys: crate::diagnostics::TmuxOptionFact::Unavailable,
                set_clipboard: crate::diagnostics::TmuxOptionFact::Unavailable,
                allow_passthrough_support: crate::diagnostics::TmuxSupportFact::Unavailable,
                allow_passthrough: crate::diagnostics::TmuxOptionFact::Unavailable,
            },
            color: ColorFacts {
                level: RuntimeFact::Available(ColorLevel::TrueColor),
                available_themes: crate::theme::ThemeKind::ALL.to_vec(),
                total_themes: crate::theme::ThemeKind::ALL.len(),
            },
            keyboard: Some(KeyboardFact {
                modifier_delivery: ModifierDelivery::new_for_test(
                    ModifierFate::Dropped,
                    ModifierFate::Native,
                ),
                os: HostOs::Macos,
            }),
            newline: None,
            clipboard: ClipboardFacts {
                native_route: true,
                native_tool: "pbcopy".to_owned(),
                native_preflight: crate::clipboard::NativeClipboardPreflight::LocalAvailable,
                tmux_route: false,
                osc52_route: false,
                osc52_capability: crate::clipboard::Osc52Capability::Supported,
                wrap_sink: false,
                display_server: crate::host::DisplayServer::Unknown,
                container_no_display: false,
                data_control: DataControlFact::NotApplicable,
                delivery: crate::clipboard::ClipboardDelivery::Confirmed,
                fix: None,
            },
            voice: None,
        },
        findings: Vec::new(),
        probe_notes: Vec::new(),
    };

    assert_eq!(
        format_doctor(&report),
        concat!(
            "环境\n",
            "  terminal     WezTerm\n",
            "  multiplexer  None detected\n",
            "  ssh          no\n",
            "  color        truecolor\n",
            "  themes       all\n",
            "  keyboard     cmd=dropped, opt=native (系统救援已启用)\n",
            "\n",
            "剪贴板\n",
            "  native       local (pbcopy)\n",
            "  tmux         off\n",
            "  osc 52       off\n",
            "  wrap         off\n",
            "  status       confirmed\n",
            "\n",
            "未发现问题。\n",
        )
    );
}
