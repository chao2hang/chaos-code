//! In-TUI `/doctor` report formatting.

use super::{
    DataControlFact, DiagnosticReport, FindingDisposition, NewlineFact, RuntimeFact, VoiceFacts,
};
use crate::clipboard::{ClipboardDelivery, NativeClipboardPreflight};
use crate::host::{DisplayServer, HostOs};

pub fn format_doctor(report: &DiagnosticReport) -> String {
    let facts = &report.facts;
    let mut out = String::new();
    out.push_str("环境\n");
    out.push_str(&format!("  terminal     {}\n", facts.terminal));
    if let RuntimeFact::Available(xtversion) = &facts.xtversion {
        out.push_str(&format!("  xtversion    {xtversion}\n"));
    }
    out.push_str(&format!("  multiplexer  {}\n", facts.multiplexer));
    if let Some(byobu) = facts.byobu {
        out.push_str(&format!("  byobu        {byobu}\n"));
    }
    out.push_str(&format!(
        "  ssh          {}\n",
        if facts.ssh { "yes" } else { "no" }
    ));
    let color_level = match &facts.color.level {
        RuntimeFact::Available(level) => Some(*level),
        RuntimeFact::NoReply | RuntimeFact::Unavailable => None,
    };
    if let Some(color_level) = color_level {
        out.push_str(&format!("  color        {}\n", color_level.as_str()));
    }
    if color_level.is_some() && facts.color.available_themes.len() == facts.color.total_themes {
        out.push_str("  themes       all\n");
    } else if color_level.is_some() {
        let themes = facts
            .color
            .available_themes
            .iter()
            .map(|theme| theme.display_name())
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "  themes       {}/{}: {themes}\n",
            facts.color.available_themes.len(),
            facts.color.total_themes
        ));
    }
    if let Some(keyboard) = &facts.keyboard {
        let rescue = if keyboard.os == HostOs::Macos {
            "系统救援已启用"
        } else {
            "当前平台无系统救援"
        };
        out.push_str(&format!(
            "  keyboard     {} ({rescue})\n",
            keyboard.modifier_delivery.label()
        ));
    }
    if let Some(newline) = &facts.newline {
        let detail = match newline {
            NewlineFact::Vte {
                version: Some(version),
            } => format!("VTE {version}；Shift+Enter 需 >= 8200"),
            NewlineFact::Vte { version: None } => {
                "旧版 VTE；Shift+Enter 需 VTE >= 0.82".to_owned()
            }
            NewlineFact::XtermJs { terminal } => {
                format!("{terminal}：xterm.js 无法区分 Shift+Enter")
            }
            NewlineFact::NoKittyKeyboardProtocol => {
                "无 Kitty 键盘协议；Shift+Enter 等同 Enter".to_owned()
            }
        };
        out.push_str(&format!("  newline      Alt+Enter（{detail}）\n"));
    }

    let clipboard = &facts.clipboard;
    let native = match clipboard.native_preflight {
        NativeClipboardPreflight::LocalAvailable => {
            format!("local ({})", clipboard.native_tool)
        }
        NativeClipboardPreflight::RemoteOnly if clipboard.container_no_display => {
            format!("container ({})", clipboard.native_tool)
        }
        NativeClipboardPreflight::RemoteOnly => {
            format!("remote ({})", clipboard.native_tool)
        }
        NativeClipboardPreflight::Unavailable => "unavailable".to_owned(),
        NativeClipboardPreflight::Disabled => "off".to_owned(),
    };
    out.push_str("\n剪贴板\n");
    out.push_str(&format!("  native       {native}\n"));
    out.push_str(&format!(
        "  tmux         {}\n",
        if clipboard.tmux_route { "on" } else { "off" }
    ));
    out.push_str(&format!(
        "  osc 52       {}\n",
        if clipboard.osc52_route {
            clipboard.osc52_capability.label()
        } else {
            "off"
        }
    ));
    out.push_str(&format!(
        "  wrap         {}\n",
        if clipboard.wrap_sink { "on" } else { "off" }
    ));
    if clipboard.display_server == DisplayServer::Wayland {
        out.push_str(&format!(
            "  data-control {}\n",
            if clipboard.data_control == DataControlFact::Available {
                "on"
            } else {
                "off"
            }
        ));
    }
    let status = match clipboard.delivery {
        ClipboardDelivery::Confirmed => "confirmed",
        ClipboardDelivery::Unverified => "unverified",
        ClipboardDelivery::Failed => "unavailable",
    };
    out.push_str(&format!("  status       {status}\n"));

    if let Some(voice) = &facts.voice {
        out.push_str("\n语音\n");
        match voice {
            VoiceFacts::Device { name, detail } => {
                out.push_str(&format!("  microphone   {name} ({detail})\n"));
            }
            VoiceFacts::Missing { .. } => {
                out.push_str("  microphone   未检测到\n");
            }
        }
    }

    format_findings(report, &mut out);
    out
}

fn format_findings(report: &DiagnosticReport, out: &mut String) {
    let issues = report
        .findings
        .iter()
        .filter(|finding| finding.disposition == FindingDisposition::Issue)
        .collect::<Vec<_>>();
    if issues.is_empty() {
        if report.issue_count() == 0 {
            out.push_str("\n未发现问题。\n");
        } else {
            out.push_str("\n问题已显示在上方剪贴板状态中。\n");
        }
    } else {
        out.push_str(&format!("\n问题 ({})\n", issues.len()));
        for finding in issues {
            format_finding(out, finding);
        }
    }

    let recommendations = report
        .findings
        .iter()
        .filter(|finding| finding.disposition == FindingDisposition::Recommendation)
        .collect::<Vec<_>>();
    if !recommendations.is_empty() {
        out.push_str("\n建议\n");
        for finding in recommendations {
            format_finding(out, finding);
        }
    }
}

fn format_finding(out: &mut String, finding: &super::DiagnosticFinding) {
    let marker = match finding.disposition {
        FindingDisposition::Issue => "!",
        FindingDisposition::Recommendation => "i",
    };
    out.push_str(&format!(
        "\n  {marker} {}  {}\n",
        finding.id, finding.message
    ));
    if let Some(automatic) = finding.automatic_remediation {
        let command = super::human_fix_command(automatic.fix_id)
            .unwrap_or_else(|| automatic.command.to_owned());
        out.push_str(&format!("      自动修复：`{command}`\n"));
    }
    if let Some(remediation) = &finding.remediation {
        match (&remediation.config_path, &finding.automatic_remediation) {
            (Some(path), _) => {
                out.push_str(&format!("      在 {path} 中添加 `{}`\n", remediation.fix));
            }
            (None, Some(_)) => {
                out.push_str(&format!("      一次性：`{}`\n", remediation.fix));
            }
            (None, None) => {
                out.push_str(&format!("      运行：`{}`\n", remediation.fix));
            }
        }
    }
    if let Some(note) = &finding.note {
        out.push_str(&format!("      说明：{note}\n"));
    }
}

#[cfg(test)]
#[path = "doctor_format_tests.rs"]
mod tests;
