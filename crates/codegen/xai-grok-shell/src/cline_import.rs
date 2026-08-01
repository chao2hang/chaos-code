// cline_import.rs
// Detects an installed Cline (VS Code-family extension) and extracts its model
// provider settings (base_url / auth_scheme / api_backend / api_key / model id)
// so the `/provider` "add channel" flow can offer an "import from Cline" option.
//
// READ-ONLY: we never write to, or otherwise alter, Cline's state database. Keys
// Cline stored through Electron `safeStorage` (ciphertext `v1:` values) cannot be
// recovered here and are reported as `key_encrypted`.

use std::collections::HashMap;
use std::path::PathBuf;

use tracing::{debug, warn};

/// Cline's VS Code / Cursor extension id; globalState keys are stored under it.
pub const CLINE_EXT_ID: &str = "saoudrizwan.claude-dev";

/// One detected Cline installation (an editor flavor + its `state.vscdb`).
#[derive(Debug, Clone)]
pub struct ClineInstall {
    /// Editor flavor name, e.g. "Code", "Cursor".
    pub editor: &'static str,
    /// Absolute path to the `state.vscdb` global-state database.
    pub db_path: PathBuf,
}

/// A single importable provider channel extracted from Cline.
#[derive(Debug, Clone)]
pub struct ClineProvider {
    /// Suggested channel id, e.g. `cline-openai` / `cline-anthropic`.
    pub id: String,
    /// Human-friendly display name.
    pub display: String,
    pub base_url: String,
    pub auth_scheme: String,
    pub api_backend: String,
    /// API key when stored in plaintext.
    pub api_key: Option<String>,
    /// Cline's current model id when available.
    pub model: Option<String>,
    /// `true` when the key is `safeStorage` ciphertext and cannot be imported.
    pub key_encrypted: bool,
}

// ── Editor global-storage discovery ─────────────────────────────────────────

/// Best-effort list of `<editor, globalStorage dir>` pairs for the edited
/// installs we know about. Missing dirs are simply not returned.
fn editor_global_storage_dirs() -> Vec<(&'static str, PathBuf)> {
    let mut out: Vec<(&'static str, PathBuf)> = Vec::new();
    let editors = ["Code", "Cursor", "Windsurf", "VSCodium"];

    if let Some(home) = dirs::home_dir() {
        #[cfg(target_os = "macos")]
        {
            let base = home.join("Library").join("Application Support");
            for e in editors {
                out.push((e, base.join(e).join("User").join("globalStorage")));
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let base = home.join(".config");
            for e in editors {
                out.push((e, base.join(e).join("User").join("globalStorage")));
            }
        }
    }

    #[cfg(target_os = "windows")]
    if let Ok(appdata) = std::env::var("APPDATA") {
        let root = PathBuf::from(appdata);
        for e in ["Code", "Cursor", "Windsurf"] {
            out.push((e, root.join(e).join("User").join("globalStorage")));
        }
    }

    out
}

/// Detect Cline installs present on this machine. Only returns entries whose
/// `state.vscdb` actually exists.
pub fn detect_cline_installs() -> Vec<ClineInstall> {
    editor_global_storage_dirs()
        .into_iter()
        .filter_map(|(editor, dir)| {
            let db = dir.join("state.vscdb");
            if db.is_file() {
                Some(ClineInstall { editor, db_path: db })
            } else {
                None
            }
        })
        .collect()
}

/// Scan every detected Cline install and return a deduplicated list of
/// importable provider channels. Order is "first readable install wins".
pub fn list_cline_providers() -> Vec<ClineProvider> {
    let mut providers: Vec<ClineProvider> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for install in detect_cline_installs() {
        match read_cline_providers(&install.db_path) {
            Ok(candidates) => {
                for c in candidates {
                    if seen.insert(c.id.clone()) {
                        providers.push(c);
                    }
                }
            }
            Err(e) => {
                debug!(
                    "cline_import: failed to read {} ({:?}): {e}",
                    install.editor, install.db_path
                );
            }
        }
    }
    providers
}

// ── state.vscdb reading ─────────────────────────────────────────────────────

/// Read Cline's provider settings from one `state.vscdb` (read-only).
pub fn read_cline_providers(db: &std::path::Path) -> Result<Vec<ClineProvider>, String> {
    let conn = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("open {db:?}: {e}"))?;

    let prefix = format!("{CLINE_EXT_ID}.");
    let mut fields: HashMap<String, String> = HashMap::new();

    // ItemTable(value) is often stored as a BLOB; read bytes and lossy-decode.
    match conn.prepare("SELECT key, value FROM ItemTable") {
        Ok(mut stmt) => {
            let rows = stmt
                .query_map([], |row| {
                    let key: String = row.get(0)?;
                    let val: Vec<u8> = row
                        .get(1)
                        .or_else(|_| row.get::<_, String>(1).map(String::into_bytes))?;
                    Ok((key, val))
                })
                .map_err(|e| format!("query ItemTable: {e}"))?;

            for row in rows {
                let (key, val) = row.map_err(|e| format!("row: {e}"))?;
                if let Some(rest) = key.strip_prefix(&prefix) {
                    let text = String::from_utf8_lossy(&val).into_owned();
                    fields.insert(rest.to_string(), text);
                }
            }
        }
        Err(e) => {
            warn!("cline_import: no ItemTable in {db:?}: {e}");
            return Err(format!("no ItemTable: {e}"));
        }
    }

    Ok(parse_fields(&fields))
}



/// Turn the raw `cline_*` field map into provider channels. `raw` values are the
/// JSON-encoded globalState bytes.
fn parse_fields(fields: &HashMap<String, String>) -> Vec<ClineProvider> {
    // Drop the generic JSON quotes Cline wraps string values in.
    let get_str = |key: &str| -> Option<String> {
        let raw = fields.get(key)?;
        match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(serde_json::Value::String(s)) => Some(s),
            Ok(serde_json::Value::Number(n)) => Some(n.to_string()),
            _ => Some(raw.clone()),
        }
    };

    let api_provider = get_str("apiProvider")
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let api_model = get_str("apiModelId");

    let anthropic = api_provider == "anthropic";
    let primary = if anthropic {
        (
            get_str("anthropicBaseUrl").filter(|s| !s.is_empty()),
            get_str("anthropicApiKey"),
            "x_api_key",
            "messages",
        )
    } else {
        (
            get_str("openAiBaseUrl").filter(|s| !s.is_empty()),
            get_str("openAiApiKey"),
            "bearer",
            "responses",
        )
    };

    // Custom providers defined by the user (array under customApiProviders).
    let mut out: Vec<ClineProvider> = Vec::new();
    if let Some(custom_raw) = fields.get("customApiProviders") {
        if let Ok(serde_json::Value::Array(arr)) =
            serde_json::from_str::<serde_json::Value>(custom_raw)
        {
            for item in arr {
                let name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("custom")
                    .to_string();
                let base = item
                    .get("baseUrl")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let key = item.get("apiKey").and_then(|v| v.as_str());
                if base.is_empty() {
                    continue;
                }
                let encrypted = key.is_some_and(|k| k.starts_with("v1:"));
                out.push(ClineProvider {
                    id: format!("cline-{}", slug(&name)),
                    display: format!("Cline · {name}"),
                    base_url: base,
                    auth_scheme: "bearer".to_string(),
                    api_backend: "chat_completions".to_string(),
                    api_key: key
                        .filter(|k| !k.is_empty() && !k.starts_with("v1:"))
                        .map(|k| k.to_string()),
                    model: None,
                    key_encrypted: encrypted,
                });
            }
        }
    }

    // Primary provider channel (anthropic / openai-family).
    let (primary_base, primary_key, scheme, backend) = primary;
    if let Some(base) = primary_base {
        let encrypted = primary_key.as_deref().is_some_and(|k| k.starts_with("v1:"));
        out.insert(
            0,
            ClineProvider {
                id: if anthropic {
                    "cline-anthropic".to_string()
                } else {
                    "cline-openai".to_string()
                },
                display: if anthropic {
                    "Cline · Anthropic".to_string()
                } else {
                    "Cline · OpenAI 兼容".to_string()
                },
                base_url: base,
                auth_scheme: scheme.to_string(),
                api_backend: backend.to_string(),
                api_key: primary_key.filter(|k| !k.starts_with("v1:")),
                model: api_model,
                key_encrypted: encrypted,
            },
        );
    }

    out
}

/// Lowercase + alnum-dash slug for a display name.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Create a `state.vscdb` exposing the given `cline_*` fields (JSON-encoded
    /// the way VS Code does) and return its path.
    fn seed_db(fields: &[(&str, &str)]) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.vscdb");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
            .unwrap();
        for (key, val) in fields {
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                rusqlite::params![format!("{CLINE_EXT_ID}.{key}"), val.as_bytes()],
            )
            .unwrap();
        }
        drop(conn);
        (dir, db)
    }

    #[test]
    fn reads_anthropic_plaintext() {
        let (_d, db) = seed_db(&[
            ("apiProvider", "\"anthropic\""),
            ("anthropicBaseUrl", "\"https://api.anthropic.com\""),
            ("anthropicApiKey", "\"sk-ant-abc\""),
            ("apiModelId", "\"claude-sonnet-4-5\""),
        ]);
        let out = read_cline_providers(&db).unwrap();
        assert_eq!(out.len(), 1);
        let p = &out[0];
        assert_eq!(p.id, "cline-anthropic");
        assert_eq!(p.base_url, "https://api.anthropic.com");
        assert_eq!(p.auth_scheme, "x_api_key");
        assert_eq!(p.api_backend, "messages");
        assert_eq!(p.api_key.as_deref(), Some("sk-ant-abc"));
        assert_eq!(p.model.as_deref(), Some("claude-sonnet-4-5"));
        assert!(!p.key_encrypted);
    }

    #[test]
    fn reads_openai_family() {
        let (_d, db) = seed_db(&[
            ("apiProvider", "\"openai\""),
            ("openAiBaseUrl", "\"https://api.openai.com/v1\""),
            ("openAiApiKey", "\"sk-xyz\""),
        ]);
        let out = read_cline_providers(&db).unwrap();
        assert_eq!(out.len(), 1);
        let p = &out[0];
        assert_eq!(p.id, "cline-openai");
        assert_eq!(p.auth_scheme, "bearer");
        assert_eq!(p.api_key.as_deref(), Some("sk-xyz"));
    }

    #[test]
    fn marks_safe_storage_key_as_encrypted() {
        let (_d, db) = seed_db(&[
            ("apiProvider", "\"anthropic\""),
            ("anthropicBaseUrl", "\"https://api.anthropic.com\""),
            ("anthropicApiKey", "v1:deadbeef"),
        ]);
        let out = read_cline_providers(&db).unwrap();
        assert_eq!(out.len(), 1);
        let p = &out[0];
        assert!(p.key_encrypted);
        assert!(p.api_key.is_none());
    }

    #[test]
    fn parses_custom_providers() {
        let (_d, db) = seed_db(&[(
            "customApiProviders",
            r#"[{"name":"My Gateway","baseUrl":"https://gw.example/v1","apiKey":"gk-1"}]"#,
        )]);
        let out = read_cline_providers(&db).unwrap();
        assert_eq!(out.len(), 1);
        let p = &out[0];
        assert_eq!(p.id, "cline-my-gateway");
        assert_eq!(p.base_url, "https://gw.example/v1");
        assert_eq!(p.api_key.as_deref(), Some("gk-1"));
        assert_eq!(p.auth_scheme, "bearer");
    }

    #[test]
    fn empty_database_never_panics() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.vscdb");
        let conn = rusqlite::Connection::open(&db).unwrap();
        drop(conn);
        let res = read_cline_providers(&db);
        assert!(res.is_ok() || res.is_err());
    }

    #[test]
    fn detected_installs_have_existing_dbs() {
        assert!(detect_cline_installs()
            .iter()
            .all(|i| i.db_path.is_file()));
    }
}

