use crate::clipboard::{ClipboardDelivery, NativeClipboardPreflight};
use crate::diagnostics::{
    DataControlFact, DiagnosticFinding, DiagnosticReport, FindingDisposition, NewlineFact,
    ProbeStatus, RuntimeFact, VoiceFacts,
};
use crate::host::{DisplayServer, HostOs};

const LIVE_TUI_PROBE_CTA: &str = "部分检查仅在 Chaos 内运行。请启动 Chaos 并执行 /doctor。";

pub(super) fn format(report: &DiagnosticReport) -> String {
    let facts = &report.facts;
    let mut out = String::from("Chaos Doctor\n\n环境\n");

    fact(&mut out, "terminal", &facts.terminal.to_string());
    match &facts.xtversion {
        RuntimeFact::Available(value) => fact(&mut out, "terminal version", value),
        RuntimeFact::NoReply => unavailable(&mut out, "terminal version", "no reply"),
        RuntimeFact::Unavailable => unavailable(&mut out, "terminal version", "unavailable"),
    }
    fact(&mut out, "multiplexer", &facts.multiplexer.to_string());
    if let Some(byobu) = facts.byobu {
        fact(&mut out, "byobu", &byobu.to_string());
    }
    fact(&mut out, "ssh", if facts.ssh { "yes" } else { "no" });
    match &facts.color.level {
        RuntimeFact::Available(level) => {
            fact(&mut out, "color", level.as_str());
            let themes = if facts.color.available_themes.len() == facts.color.total_themes {
                "all".to_owned()
            } else {
                format!(
                    "{}/{}: {}",
                    facts.color.available_themes.len(),
                    facts.color.total_themes,
                    facts
                        .color
                        .available_themes
                        .iter()
                        .map(|theme| theme.display_name())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            fact(&mut out, "themes", &themes);
        }
        RuntimeFact::NoReply | RuntimeFact::Unavailable => {
            unavailable(&mut out, "color", "unavailable");
            unavailable(&mut out, "themes", "unavailable");
        }
    }

    if let Some(keyboard) = &facts.keyboard {
        let rescue = if keyboard.os == HostOs::Macos {
            "系统救援已启用"
        } else {
            "当前平台无系统救援"
        };
        fact(
            &mut out,
            "keyboard",
            &format!("{} ({rescue})", keyboard.modifier_delivery.label()),
        );
    }
    if let Some(newline) = &facts.newline {
        fact(&mut out, "newline", &format_newline(newline));
    }

    let clipboard = &facts.clipboard;
    let native = match clipboard.native_preflight {
        NativeClipboardPreflight::LocalAvailable => {
            format!("local ({})", clipboard.native_tool)
        }
        NativeClipboardPreflight::RemoteOnly if clipboard.container_no_display => {
            format!("container ({})", clipboard.native_tool)
        }
        NativeClipboardPreflight::RemoteOnly => format!("remote ({})", clipboard.native_tool),
        NativeClipboardPreflight::Unavailable => "unavailable".to_owned(),
        NativeClipboardPreflight::Disabled => "off".to_owned(),
    };
    out.push_str("\n剪贴板\n");
    fact(&mut out, "native", &native);
    fact(
        &mut out,
        "tmux",
        if clipboard.tmux_route { "on" } else { "off" },
    );
    fact(
        &mut out,
        "osc 52",
        if clipboard.osc52_route {
            clipboard.osc52_capability.label()
        } else {
            "off"
        },
    );
    fact(
        &mut out,
        "SSH wrap",
        if clipboard.wrap_sink { "on" } else { "off" },
    );
    if clipboard.display_server == DisplayServer::Wayland {
        match clipboard.data_control {
            DataControlFact::Available => fact(&mut out, "data-control", "on"),
            DataControlFact::Missing => fact(&mut out, "data-control", "off"),
            DataControlFact::Unavailable => unavailable(&mut out, "data-control", "unavailable"),
            DataControlFact::Error => {
                let detail = report
                    .probe_notes
                    .iter()
                    .find(|note| note.probe == "wayland.data-control")
                    .and_then(|note| note.message.as_deref());
                match detail {
                    Some(message) => {
                        unavailable(&mut out, "data-control", &format!("error: {message}"))
                    }
                    None => unavailable(&mut out, "data-control", "error"),
                }
            }
            DataControlFact::NotApplicable => {}
        }
    }
    let status = match clipboard.delivery {
        ClipboardDelivery::Confirmed => "confirmed",
        ClipboardDelivery::Unverified => "unverified",
        ClipboardDelivery::Failed => "unavailable",
    };
    fact(&mut out, "status", status);

    if let Some(voice) = &facts.voice {
        out.push_str("\n语音\n");
        match voice {
            VoiceFacts::Device { name, detail } => {
                fact(&mut out, "microphone", &format!("{name} ({detail})"));
            }
            VoiceFacts::Missing { error } => {
                fact(&mut out, "microphone", &format!("未检测到 ({error})"));
            }
        }
    }

    if !report.findings.is_empty() {
        out.push_str("\n发现\n");
        for finding in &report.findings {
            format_finding(&mut out, finding);
        }
    }

    let visible_notes = report
        .probe_notes
        .iter()
        .filter(|note| !fact_already_shows_probe(note.probe));
    let mut notes = visible_notes.peekable();
    if notes.peek().is_some() {
        out.push_str("\n未完成检查\n");
        for note in notes {
            let message = match &note.message {
                Some(message) => format!("{}: {message}", probe_status(note.status)),
                None => probe_status(note.status).to_owned(),
            };
            row(&mut out, "?", note.probe, &message);
        }
    }

    if report
        .probe_notes
        .iter()
        .any(crate::diagnostics::probe_requires_live_tui)
    {
        out.push_str("\n需要运行中的会话\n");
        out.push_str(&format!("  {LIVE_TUI_PROBE_CTA}\n"));
    }

    let issues = report.issue_count();
    let recommendations = report.recommendation_count();
    out.push('\n');
    out.push_str(&format!(
        "{} {}，{} {}\n",
        issues,
        plural(issues, "个问题", "个问题"),
        recommendations,
        plural(recommendations, "条建议", "条建议")
    ));
    out
}

fn fact_already_shows_probe(probe: &str) -> bool {
    matches!(
        probe,
        "runtime.xtversion" | "terminal.color" | "wayland.data-control"
    )
}

fn fact(out: &mut String, label: &str, value: &str) {
    row(out, "·", label, value);
}

fn unavailable(out: &mut String, label: &str, value: &str) {
    row(out, "?", label, value);
}

fn row(out: &mut String, marker: &str, label: &str, value: &str) {
    out.push_str(&format!("  {marker} {label:<28} {value}\n"));
}

fn format_finding(out: &mut String, finding: &DiagnosticFinding) {
    let marker = match finding.disposition {
        FindingDisposition::Issue => "!",
        FindingDisposition::Recommendation => "i",
    };
    row(out, marker, &finding.id.to_string(), &finding.message);
    if let Some(automatic) = finding.automatic_remediation {
        let command = crate::diagnostics::human_fix_command(automatic.fix_id)
            .unwrap_or_else(|| automatic.command.to_owned());
        out.push_str(&format!("    → 自动修复：`{command}`\n"));
    }
    if let Some(remediation) = &finding.remediation {
        let instruction = match (&remediation.config_path, &finding.automatic_remediation) {
            (Some(path), _) => format!("在 {path} 中添加 `{}`", remediation.fix),
            (None, Some(_)) => format!("一次性：`{}`", remediation.fix),
            (None, None) => format!("运行：`{}`", remediation.fix),
        };
        out.push_str(&format!("    → {instruction}\n"));
    }
    if let Some(note) = &finding.note {
        out.push_str(&format!("      {note}\n"));
    }
}

fn format_newline(newline: &NewlineFact) -> String {
    let detail = match newline {
        NewlineFact::Vte {
            version: Some(version),
        } => format!("VTE {version}；Shift+Enter 需 >= 8200"),
        NewlineFact::Vte { version: None } => "旧版 VTE；Shift+Enter 需 VTE >= 0.82".to_owned(),
        NewlineFact::XtermJs { terminal } => {
            format!("{terminal}：xterm.js 无法区分 Shift+Enter")
        }
        NewlineFact::NoKittyKeyboardProtocol => {
            "无 Kitty 键盘协议；Shift+Enter 等同 Enter".to_owned()
        }
    };
    format!("Alt+Enter（{detail}）")
}

fn plural<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

fn probe_status(status: ProbeStatus) -> &'static str {
    match status {
        ProbeStatus::Unsupported => "unsupported",
        ProbeStatus::Unavailable => "unavailable",
        ProbeStatus::Error => "error",
    }
}
