use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
struct TelemetryStatus {
    config_root: String,
    subsystems: Subsystems,
}

#[derive(Serialize)]
struct Subsystems {
    telemetry: ResolvedMode,
    mixpanel: MixpanelStatus,
    trace_upload: ResolvedBool,
    external_otel: ExternalOtelStatus,
}

#[derive(Serialize)]
struct ResolvedMode {
    mode: String,
    source: String,
}

#[derive(Serialize)]
struct ResolvedBool {
    enabled: bool,
    source: String,
}

#[derive(Serialize)]
struct MixpanelStatus {
    enabled: bool,
    reason: &'static str,
}

#[derive(Serialize)]
struct ExternalOtelStatus {
    enabled: bool,
    source: &'static str,
    metrics_exporter: Option<&'static str>,
    logs_exporter: Option<&'static str>,
}

pub(crate) fn run(json: bool) -> Result<()> {
    let config = xai_grok_shell::config::load_agent_config_disk_only()
        .map_err(|error| anyhow::anyhow!("无法读取有效配置：{error}"))?;

    let mode = config.resolve_telemetry_mode();
    let trace_upload = config.resolve_trace_upload();
    let config_root = xai_grok_config::user_grok_home()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unavailable".to_owned());
    let mixpanel_enabled = config.telemetry.mixpanel_enabled
        && config
            .telemetry
            .mixpanel_token
            .as_deref()
            .is_some_and(|token| !token.is_empty());
    let external_otel = xai_grok_shell::agent::config::resolve_external_otel_config(
        xai_grok_telemetry::external::config::ExternalClientInfo::default(),
    );

    let report = TelemetryStatus {
        config_root,
        subsystems: Subsystems {
            telemetry: ResolvedMode {
                mode: match mode.value {
                    xai_grok_shell::agent::config::TelemetryMode::Disabled => "disabled",
                    xai_grok_shell::agent::config::TelemetryMode::SessionMetrics => {
                        "session_metrics"
                    }
                    xai_grok_shell::agent::config::TelemetryMode::Enabled => "enabled",
                }
                .to_owned(),
                source: mode.source.to_string(),
            },
            mixpanel: MixpanelStatus {
                enabled: mixpanel_enabled,
                reason: if mixpanel_enabled {
                    "configured"
                } else {
                    "disabled_or_no_token"
                },
            },
            trace_upload: ResolvedBool {
                enabled: trace_upload.value,
                source: trace_upload.source.to_string(),
            },
            external_otel: ExternalOtelStatus {
                enabled: external_otel.is_some(),
                source: external_otel
                    .as_ref()
                    .map_or("disabled_or_not_configured", |config| config.enabled_source),
                metrics_exporter: external_otel.as_ref().and_then(|config| {
                    match config.metrics_exporter {
                        xai_grok_telemetry::external::config::ExporterSelection::Otlp => {
                            Some("otlp")
                        }
                        xai_grok_telemetry::external::config::ExporterSelection::Console => {
                            Some("console")
                        }
                        xai_grok_telemetry::external::config::ExporterSelection::None => None,
                    }
                }),
                logs_exporter: external_otel.as_ref().and_then(|config| {
                    match config.logs_exporter {
                        xai_grok_telemetry::external::config::ExporterSelection::Otlp => {
                            Some("otlp")
                        }
                        xai_grok_telemetry::external::config::ExporterSelection::Console => {
                            Some("console")
                        }
                        xai_grok_telemetry::external::config::ExporterSelection::None => None,
                    }
                }),
            },
        },
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("config root: {}", report.config_root);
        println!(
            "telemetry: {} (source: {})",
            report.subsystems.telemetry.mode, report.subsystems.telemetry.source
        );
        println!(
            "mixpanel: {} ({})",
            enabled_text(report.subsystems.mixpanel.enabled),
            report.subsystems.mixpanel.reason
        );
        println!(
            "trace upload: {} (source: {})",
            enabled_text(report.subsystems.trace_upload.enabled),
            report.subsystems.trace_upload.source
        );
        println!(
            "external OTEL: {} (source: {})",
            enabled_text(report.subsystems.external_otel.enabled),
            report.subsystems.external_otel.source
        );
        if let Some(exporter) = report.subsystems.external_otel.metrics_exporter {
            println!("  metrics exporter: {exporter}");
        }
        if let Some(exporter) = report.subsystems.external_otel.logs_exporter {
            println!("  logs exporter: {exporter}");
        }
    }
    Ok(())
}

fn enabled_text(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}
