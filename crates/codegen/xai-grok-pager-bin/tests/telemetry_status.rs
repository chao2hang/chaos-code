use std::process::{Command, Output};

fn chaos_binary() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("PAGER_BINARY") {
        return std::path::absolute(&path)
            .unwrap_or_else(|error| panic!("cannot resolve PAGER_BINARY {path}: {error}"));
    }
    option_env!("CARGO_BIN_EXE_chaos")
        .or(option_env!("CARGO_BIN_EXE_xai-grok-pager"))
        .map(std::path::PathBuf::from)
        .expect("Cargo must provide the chaos binary path")
}

fn run_status(config: &str, args: &[&str], external_otel: bool) -> (Output, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("temporary config root");
    std::fs::write(home.path().join("config.toml"), config).expect("write isolated config");
    let output = Command::new(chaos_binary())
        .args(["telemetry", "status"])
        .args(args)
        .env_clear()
        .env("CHAOS_HOME", home.path())
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GROK_EXTERNAL_OTEL", if external_otel { "1" } else { "0" })
        .env(
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "https://secret-endpoint.invalid/v1",
        )
        .env(
            "OTEL_METRICS_EXPORTER",
            if external_otel { "otlp" } else { "none" },
        )
        .output()
        .expect("run shipped chaos binary");
    (output, home)
}

#[test]
fn json_status_reports_resolved_config_without_secrets_or_endpoints() {
    let token = "private-test-token-must-not-print";
    let endpoint = "https://private-collector.invalid/v1/traces";
    let config = format!(
        "[features]\ntelemetry = \"session_metrics\"\n\n[telemetry]\ntrace_upload = false\nmixpanel_enabled = true\nmixpanel_token = \"{token}\"\notel_enabled = true\notel_endpoint = \"{endpoint}\"\n"
    );
    let (output, home) = run_status(&config, &["--json"], true);
    assert!(
        output.status.success(),
        "telemetry status exited unsuccessfully: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("JSON output is UTF-8");
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("valid status JSON");
    assert_eq!(report["config_root"], home.path().display().to_string());
    assert_eq!(report["subsystems"]["telemetry"]["mode"], "session_metrics");
    assert_eq!(report["subsystems"]["telemetry"]["source"], "config");
    assert_eq!(report["subsystems"]["mixpanel"]["enabled"], true);
    assert_eq!(report["subsystems"]["trace_upload"]["enabled"], false);
    assert_eq!(report["subsystems"]["external_otel"]["enabled"], true);
    assert_eq!(
        report["subsystems"]["external_otel"]["metrics_exporter"],
        "otlp"
    );
    assert_eq!(
        report["subsystems"]["external_otel"]["logs_exporter"],
        serde_json::Value::Null
    );
    let serialized = report.to_string();
    assert!(
        !serialized.contains(token),
        "status leaked the Mixpanel token"
    );
    assert!(
        !serialized.contains(endpoint),
        "status leaked the collector endpoint"
    );
    assert!(
        !serialized.contains("https://secret-endpoint.invalid/v1"),
        "status leaked the environment collector endpoint"
    );
    assert!(
        !serialized.contains("headers"),
        "status leaked OTEL headers"
    );
}

#[test]
fn human_status_is_readable_and_corrupt_config_fails_closed() {
    let (output, _home) = run_status(
        "[features]\ntelemetry = \"disabled\"\n[telemetry]\ntrace_upload = false\n",
        &[],
        false,
    );
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("human output is UTF-8");
    assert!(
        stdout.contains("telemetry: disabled (source: config)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("trace upload: disabled (source: config)"),
        "{stdout}"
    );

    let (bad_config, _home) = run_status("this is not valid TOML [[[", &["--json"], false);
    assert!(
        !bad_config.status.success(),
        "corrupt config must not produce a false status"
    );
    assert!(
        String::from_utf8_lossy(&bad_config.stderr).contains("无法读取有效配置"),
        "{}",
        String::from_utf8_lossy(&bad_config.stderr)
    );
}
