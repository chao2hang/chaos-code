//! Read-only view of the Chaos CLI's own session store.
//!
//! The terminal client (`chaos` started in a project directory) writes every
//! conversation it has to `$CHAOS_HOME/sessions/<url-encoded cwd>/<session id>/`:
//! `summary.json` carries the metadata (cwd, title, counts, timestamps) and
//! `chat_history.jsonl` carries the model-facing transcript. That store is the
//! fact source for "which conversations exist, and in which directory" — the
//! GUI's own state file is a second, younger copy of the same idea.
//!
//! This module reads that store so the Web/ZCode front end can show and continue
//! conversations the terminal had, in the directory they happened in, instead of
//! only the ones the browser itself started. Two rules hold the seam in place:
//!
//! * **Read-only.** Nothing here opens a file for writing. The continuation of a
//!   conversation is produced by the CLI itself (see
//!   [`crate::RunOptions::resume_session`]), which appends to its own transcript;
//!   a GUI that rewrote those files would fork a conversation in two places.
//! * **Directory names are a claim, not the truth.** The parent directory is
//!   `urlencoding::encode(cwd)`, and the machine's real store on this box shows
//!   that the field inside `summary.json` (`info.cwd`) is the authoritative one:
//!   the same session can be relocated out of a directory whose encoded name no
//!   longer matches it. `cwd` therefore prefers `info.cwd` and falls back to the
//!   decoded directory name only when the summary does not say.

use crate::TimelineMessage;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// The subdirectory of the Chaos home that holds the per-directory sessions.
pub const SESSIONS_SUBDIR: &str = "sessions";

/// A `summary.json` is a small metadata record. Reading more than this means the
/// file is not what it claims to be, and a session whose summary cannot be read
/// is skipped rather than shown with invented metadata.
const SUMMARY_READ_LIMIT: u64 = 512 * 1024;

/// How much of a `chat_history.jsonl` tail to read when hydrating a transcript.
/// Long conversations keep their recent turns, which is what an opened panel and
/// a continued prompt need; the whole file stays on disk.
const TRANSCRIPT_READ_LIMIT: u64 = 4 * 1024 * 1024;

/// How far into a transcript to read looking for the first human turn, used only
/// to title a session whose `summary.json` has no summary yet.
const TITLE_PEEK_LIMIT: u64 = 256 * 1024;

/// Message cap for one hydrated transcript, so opening a session in a months-old
/// conversation costs a bounded read and a bounded panel.
const TRANSCRIPT_MAX_MESSAGES: usize = 300;

/// Per-message text cap. A single pasted log can be megabytes; a panel that
/// renders it whole is unusable, so it is marked as cut instead.
const MESSAGE_TEXT_LIMIT: usize = 16 * 1024;

/// Session cap for one scan, newest first.
pub const SCAN_MAX_SESSIONS: usize = 500;

/// One conversation as the CLI's own store describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChaosSessionMeta {
    /// The CLI's session id (a UUIDv7 string), unique per directory bucket.
    pub native_id: String,
    /// The directory this conversation belongs to.
    pub cwd: String,
    /// The on-disk directory holding this session's files.
    pub directory: PathBuf,
    /// The title the terminal would show: `generated_title`, else
    /// `session_summary`, else the first human turn. Never `None` for a listed
    /// session — see [`read_session_directory`] for why a nameless one is not
    /// listed at all.
    pub title: Option<String>,
    /// Chat message count as the CLI counts it.
    pub message_count: usize,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// The model the session last ran on — reported, never applied.
    pub model: Option<String>,
}

impl ChaosSessionMeta {
    /// The title to show. Set by the time a session is listed, because a session
    /// the terminal could not name was not listed at all.
    pub fn display_title(&self) -> Option<String> {
        self.title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_owned)
    }
}

/// The Chaos home the CLI uses, without creating it: `$CHAOS_HOME`, `$GROK_HOME`,
/// `~/.chaos` or the legacy `~/.grok`. `None` when no home resolves at all.
pub fn chaos_home() -> Option<PathBuf> {
    xai_dirs::user_grok_home()
}

/// Where the CLI keeps its sessions for this machine, whether or not it exists
/// yet. A host passes this to [`crate::Engine::import_chaos_sessions`] and to
/// [`scan_store`], both of which answer "nothing" for a missing directory.
pub fn sessions_dir() -> Option<PathBuf> {
    chaos_home().map(|home| home.join(SESSIONS_SUBDIR))
}

/// Reverse of the CLI's `urlencoding::encode(cwd)` directory naming.
///
/// Only `%XX` is translated: the CLI percent-encodes, and percent-encoding does
/// not turn `+` into a space, so a `+` in a path must survive as a `+`.
pub fn decode_cwd_dirname(name: &str) -> Option<String> {
    let bytes = name.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            let byte = u8::from_str_radix(hex, 16).ok()?;
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(out).ok()?;
    (!decoded.is_empty()).then_some(decoded)
}

/// Parse the CLI's RFC 3339 timestamps into epoch milliseconds. A missing or
/// unparseable timestamp is `0`, which callers treat as "unknown" rather than as
/// the epoch, so a session is never presented as older than everything else.
fn parse_time_ms(value: Option<&serde_json::Value>) -> u64 {
    value
        .and_then(serde_json::Value::as_str)
        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
        .map(|when| when.timestamp_millis().max(0) as u64)
        .unwrap_or(0)
}

fn read_bounded(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let mut limited = file.take(limit.saturating_add(1));
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes).ok()?;
    bytes.truncate(limit as usize);
    Some(bytes)
}

/// One `<user_query>…</user_query>` extraction: the CLI wraps the human turn in
/// that tag inside a larger context message, and the wrapper is noise in a panel.
fn unwrap_user_query(text: &str) -> Option<String> {
    let open = text.find("<user_query>")? + "<user_query>".len();
    let close = open + text[open..].find("</user_query>")?;
    let inner = text[open..close].trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

/// True for the context messages the CLI injects as `user` turns — the opening
/// `<user_info>` block, `<system-reminder>`s, local-command echoes. They are part
/// of the model's context, not turns a human typed.
fn is_injected_context(text: &str) -> bool {
    let head = text.trim_start();
    [
        "<user_info>",
        "<system-reminder>",
        "<local-command",
        "<command-name>",
        "<env>",
    ]
    .iter()
    .any(|prefix| head.starts_with(prefix))
}

/// The text blocks of one `chat_history.jsonl` record, joined in order.
fn content_text(record: &serde_json::Value) -> String {
    match record.get("content") {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

use serde_json::Value;

/// The first human turn of a session, reading only the head of the transcript.
pub fn first_user_text(directory: &Path) -> Option<String> {
    let bytes = read_bounded(&directory.join("chat_history.jsonl"), TITLE_PEEK_LIMIT)?;
    for line in String::from_utf8_lossy(&bytes).lines() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if record.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let text = content_text(&record);
        if text.is_empty() || is_injected_context(&text) {
            continue;
        }
        return Some(unwrap_user_query(&text).unwrap_or(text));
    }
    None
}

/// Reads one session's `summary.json` into metadata. `None` when the directory
/// is not a session (no summary, unreadable, or not JSON): a store may contain
/// strays, and skipping them beats inventing a row.
pub fn read_session_directory(directory: &Path) -> Option<ChaosSessionMeta> {
    let native_id = directory.file_name()?.to_string_lossy().to_string();
    // The terminal's own walk accepts any non-hidden directory holding a real
    // `summary.json` and never checks the name's shape. Checking it here would
    // hide sessions the CLI itself lists.
    if native_id.starts_with('.') {
        return None;
    }
    let summary_path = directory.join("summary.json");
    let bytes = read_bounded(&summary_path, SUMMARY_READ_LIMIT)?;
    let summary: serde_json::Value = serde_json::from_slice(&bytes).ok()?;

    // What the terminal's own picker shows, it shows in this order: a title the
    // model generated, then the session summary. Reading the fields in another
    // order would name conversations differently in the two clients.
    let title = ["generated_title", "session_summary"]
        .iter()
        .filter_map(|key| summary.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string);

    let kind = summary
        .get("session_kind")
        .and_then(Value::as_str)
        .unwrap_or("");
    // A subagent's internal conversation is not one the terminal lists — but
    // `hidden` says what it says, and the subagent default applies only when the
    // field is absent. An explicitly unhidden subagent session is one the CLI
    // lists, so it is one this listing lists too.
    let hidden = summary
        .get("hidden")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| kind.starts_with("subagent"));
    if hidden {
        return None;
    }
    // `grok -p` runs keep their own page in the terminal's picker. Listing them
    // here would put one-shot runs in a sidebar of conversations, and would put
    // a *duplicate* row beside every conversation this host continues: the
    // continuation runs the CLI in one-shot mode, which writes its own record.
    if kind == "headless" {
        return None;
    }

    // The encoded parent directory name is a claim; `info.cwd` is the truth.
    // The CLI's own relocation code treats it the same way, and long paths that
    // the CLI could not encode name the directory in a `.cwd` sidecar.
    let cwd = summary
        .get("info")
        .and_then(|info| info.get("cwd"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            let parent = directory.parent()?;
            std::fs::read_to_string(parent.join(".cwd"))
                .ok()
                .map(|sidecar| sidecar.trim().to_owned())
                .filter(|cwd| !cwd.is_empty())
        })
        .or_else(|| parent_of(directory).and_then(decode_cwd_dirname))?;
    if cwd.is_empty() {
        return None;
    }

    // A conversation the terminal cannot name is one the terminal does not show:
    // its list drops nameless records that hold no turns, and its own renderer
    // discards a row with no text to draw. Inventing a "新会话" row for those was
    // the difference the user saw — a sidebar of placeholder rows, and the actual
    // conversations nowhere among them.
    let title = title.or_else(|| {
        let first = first_user_text(directory)?;
        let line = first.lines().find(|line| !line.trim().is_empty())?;
        let trimmed = line.trim();
        let mut taken: String = trimmed.chars().take(60).collect();
        if trimmed.chars().count() > 60 {
            taken.push('…');
        }
        (!taken.is_empty()).then_some(taken)
    })?;
    let message_count = summary
        .get("num_chat_messages")
        .and_then(Value::as_u64)
        .or_else(|| summary.get("num_messages").and_then(Value::as_u64))
        .unwrap_or(0) as usize;
    let created_at_ms = parse_time_ms(summary.get("created_at"));
    // The picker's key is `last_active_at`, else `updated_at` — a fallback, not a
    // maximum, and never `created_at`. A metadata-only write (a rename, an agent
    // switch, a bookkeeping touch after a cwd move) moves `updated_at` without
    // the conversation having happened again, and the CLI deliberately refuses to
    // let that promote a session. Taking the max here would order "recent"
    // conversations differently in the two clients.
    let last_active = parse_time_ms(summary.get("last_active_at"));
    let updated_at_ms = if last_active != 0 {
        last_active
    } else {
        parse_time_ms(summary.get("updated_at"))
    };
    let model = summary
        .get("current_model_id")
        .and_then(Value::as_str)
        .map(str::to_string);

    Some(ChaosSessionMeta {
        native_id,
        cwd,
        directory: directory.to_path_buf(),
        title: Some(title),
        message_count,
        created_at_ms,
        updated_at_ms,
        model,
    })
}

fn parent_of(directory: &Path) -> Option<&str> {
    directory
        .parent()?
        .file_name()
        .and_then(|name| name.to_str())
}

/// Lists every conversation under `root`, newest first, capped at `limit`.
///
/// A missing `root` is "no conversations yet", not an error: a machine that has
/// never run the CLI has no store, and the host should still start.
pub fn scan_store(root: &Path, limit: usize) -> Vec<ChaosSessionMeta> {
    let Ok(cwd_dirs) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    // Every bucket, in name order. The terminal walks the whole store too, and a
    // cap applied to an unordered `read_dir` would drop whichever directories
    // happened to come first — an arbitrary, silent loss of the newest work.
    let mut buckets: Vec<PathBuf> = cwd_dirs
        .flatten()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            file_type.is_dir().then_some(entry.path())
        })
        .collect();
    buckets.sort();
    let mut found: Vec<ChaosSessionMeta> = Vec::new();
    for bucket in buckets {
        let Ok(sessions) = std::fs::read_dir(&bucket) else {
            continue;
        };
        let mut names: Vec<PathBuf> = sessions
            .flatten()
            .filter_map(|entry| {
                let file_type = entry.file_type().ok()?;
                file_type.is_dir().then_some(entry.path())
            })
            .collect();
        names.sort();
        for session in names {
            if let Some(meta) = read_session_directory(&session) {
                found.push(meta);
            }
        }
    }
    // The terminal treats one id living in two buckets as ambiguous and lists
    // neither; a listing that showed both would show one conversation twice, in
    // two directories, and let the user continue whichever one they happened to
    // click.
    let mut seen: Vec<String> = found.iter().map(|meta| meta.native_id.clone()).collect();
    seen.sort();
    let ambiguous: Vec<String> = seen
        .windows(2)
        .filter(|pair| pair[0] == pair[1])
        .map(|pair| pair[0].clone())
        .collect();
    found.retain(|meta| !ambiguous.contains(&meta.native_id));
    // Newest first, id ascending on a tie — the picker's own rule, so the same
    // two conversations sit in the same relative order in both clients.
    found.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| a.native_id.cmp(&b.native_id))
    });
    found.truncate(limit.min(SCAN_MAX_SESSIONS));
    found
}

/// The tail of a transcript, in the shape the GUI timeline and a continued
/// prompt consume: `user` / `assistant` text turns only.
///
/// Reads the *end* of the file when it is longer than the cap, because the
/// recent turns are the ones an open panel shows and the ones a follow-up needs.
/// A missing or unreadable transcript is an empty list — the session still
/// exists, it just has nothing to show.
pub fn read_transcript(directory: &Path) -> Vec<TimelineMessage> {
    let path = directory.join("chat_history.jsonl");
    let Ok(mut file) = File::open(&path) else {
        return Vec::new();
    };
    let start = match file.metadata() {
        Ok(metadata) if metadata.len() > TRANSCRIPT_READ_LIMIT => {
            // Seek back past the cap, then drop the partial line the seek landed
            // inside. `read_line`-style discards are fine: the dropped fragment
            // is at worst the oldest visible turn of a conversation this long.
            metadata.len() - TRANSCRIPT_READ_LIMIT
        }
        Ok(_) => 0,
        Err(_) => 0,
    };
    if start > 0 && file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if file
        .take(TRANSCRIPT_READ_LIMIT)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines().peekable();
    if start > 0 {
        // The first line is whatever the seek landed in the middle of.
        lines.next();
    }
    let mut messages: Vec<TimelineMessage> = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(role) = record.get("type").and_then(Value::as_str) else {
            continue;
        };
        if role != "user" && role != "assistant" {
            continue;
        }
        let raw = content_text(&record);
        if raw.trim().is_empty() {
            // Tool calls, tool results and images carry no text block; the GUI
            // has no rendering for them on this path, so they are not rows.
            continue;
        }
        let text = if role == "user" {
            if is_injected_context(&raw) {
                continue;
            }
            unwrap_user_query(&raw).unwrap_or(raw)
        } else {
            raw
        };
        let mut text = text;
        if text.chars().count() > MESSAGE_TEXT_LIMIT {
            let kept: String = text.chars().take(MESSAGE_TEXT_LIMIT).collect();
            text = kept;
            text.push_str("\n[已截断]");
        }
        messages.push(TimelineMessage {
            role: role.to_string(),
            text,
        });
    }
    if messages.len() > TRANSCRIPT_MAX_MESSAGES {
        let excess = messages.len() - TRANSCRIPT_MAX_MESSAGES;
        messages.drain(..excess);
    }
    messages
}

/// One restored V4 row from `updates.jsonl`. Text kinds merge their consecutive
/// chunks; tool calls keep their own identity so a restore shows what the
/// terminal showed.
#[derive(Clone, Debug, PartialEq)]
pub enum StoredRow {
    UserInput {
        text: String,
    },
    AssistantText {
        text: String,
    },
    Reasoning {
        text: String,
    },
    ToolCall {
        tool_call_id: String,
        tool_name: String,
        /// 官方 toolCall 行状态集的子集：还原视图只会出现
        /// `running`（没有终态更新）、`success`、`error`。
        status: &'static str,
        input_text: String,
        output_text: String,
    },
}

/// One plan entry as the terminal recorded it, status already mapped to the
/// official camelCase vocabulary.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredPlanEntry {
    pub content: String,
    pub status: &'static str,
}

/// The real per-turn token account the CLI records in `turn_completed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct StoredUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// The restored view of one session's `updates.jsonl` — the terminal's own
/// display and export source. `chat_history.jsonl` is the model-facing cache
/// and knows only text turns; tool cards, the plan and the real token account
/// exist solely here.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StoredTranscript {
    pub rows: Vec<StoredRow>,
    pub plan: Option<Vec<StoredPlanEntry>>,
    /// 最后一次 `turn_completed` 的真实账目；没有完成过的轮次就是 `None`。
    pub usage: Option<StoredUsage>,
}

/// 官方 camelCase 状态词表的一个成员。未知值（含 pending/未知字符串）如实归
/// `pending`——画成进行中或完成都是编造。
fn stored_plan_status(raw: &str) -> &'static str {
    match raw.trim().to_lowercase().as_str() {
        "completed" | "done" => "completed",
        "in_progress" | "in-progress" | "inprogress" | "running" | "active" => "inProgress",
        _ => "pending",
    }
}

/// toolCall 行终态：`completed` → `success`，`failed` → `error`，其余保持
/// `running`（还原视图没有 pendingApproval——那是活审批的形状）。
fn stored_tool_status(raw: Option<&str>) -> &'static str {
    match raw.map(str::trim).unwrap_or("") {
        "completed" => "success",
        "failed" => "error",
        _ => "running",
    }
}

/// How much of an `updates.jsonl` tail to read for a restored panel. The file
/// carries every streamed chunk, so it grows much faster than the cache; the
/// recent window is what an opened panel shows.
const UPDATES_READ_LIMIT: u64 = 4 * 1024 * 1024;

/// Row cap for one restored transcript, matching the hydrated-cache ceiling.
const UPDATES_MAX_ROWS: usize = 300;

/// Parse `updates.jsonl` in one session directory into the restored view.
/// Missing file, unparsable lines and unknown update kinds are skipped — a
/// partial restore is a fact, not an error; the caller falls back to the
/// text-only cache when this returns an empty row list.
pub fn read_updates_transcript(directory: &Path) -> StoredTranscript {
    let path = directory.join("updates.jsonl");
    let Ok(mut file) = File::open(&path) else {
        return StoredTranscript::default();
    };
    let start = match file.metadata() {
        Ok(metadata) if metadata.len() > UPDATES_READ_LIMIT => metadata.len() - UPDATES_READ_LIMIT,
        Ok(_) => 0,
        Err(_) => 0,
    };
    if start > 0 && file.seek(SeekFrom::Start(start)).is_err() {
        return StoredTranscript::default();
    }
    let mut bytes = Vec::new();
    if file
        .take(UPDATES_READ_LIMIT)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return StoredTranscript::default();
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines().peekable();
    if start > 0 {
        // The seek landed mid-line; that fragment is dropped, not half-parsed.
        lines.next();
    }

    let mut transcript = StoredTranscript::default();
    // toolCallId → row index, so a terminal update lands on its own card.
    let mut tool_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(update) = record.get("params").and_then(|params| params.get("update")) else {
            continue;
        };
        let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) else {
            continue;
        };
        match kind {
            "user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk" => {
                let Some(chunk) = update
                    .get("content")
                    .and_then(|content| content.get("text"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .filter(|text| !text.is_empty())
                else {
                    continue;
                };
                // Consecutive chunks of one kind merge into one row; a tool card
                // or a kind switch closes it, matching the live streaming shape.
                let kind_rank = match kind {
                    "user_message_chunk" => 0,
                    "agent_message_chunk" => 1,
                    _ => 2,
                };
                let last_kind = transcript.rows.last().and_then(|row| match row {
                    StoredRow::UserInput { .. } => Some(0),
                    StoredRow::AssistantText { .. } => Some(1),
                    StoredRow::Reasoning { .. } => Some(2),
                    _ => None,
                });
                if last_kind == Some(kind_rank) {
                    match transcript.rows.last_mut() {
                        Some(StoredRow::UserInput { text: existing })
                        | Some(StoredRow::AssistantText { text: existing })
                        | Some(StoredRow::Reasoning { text: existing }) => {
                            existing.push_str(&chunk);
                        }
                        _ => unreachable!("kind matched the row"),
                    }
                } else if transcript.rows.len() < UPDATES_MAX_ROWS {
                    let row = match kind {
                        "user_message_chunk" => StoredRow::UserInput { text: chunk },
                        "agent_message_chunk" => StoredRow::AssistantText { text: chunk },
                        _ => StoredRow::Reasoning { text: chunk },
                    };
                    transcript.rows.push(row);
                }
            }
            "tool_call" => {
                let Some(tool_call_id) = update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                else {
                    continue;
                };
                if transcript.rows.len() >= UPDATES_MAX_ROWS {
                    continue;
                }
                let tool_name = update
                    .get("title")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        update
                            .get("_meta")
                            .and_then(|meta| meta.get("x.ai/tool"))
                            .and_then(|tool| tool.get("name"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("tool")
                    .to_owned();
                let input_text = update
                    .get("rawInput")
                    .map(|raw| raw.to_string())
                    .unwrap_or_default();
                tool_index.insert(tool_call_id.clone(), transcript.rows.len());
                transcript.rows.push(StoredRow::ToolCall {
                    tool_call_id,
                    tool_name,
                    status: "running",
                    input_text,
                    output_text: String::new(),
                });
            }
            "tool_call_update" => {
                let Some(tool_call_id) = update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                else {
                    continue;
                };
                let Some(&index) = tool_index.get(&tool_call_id) else {
                    continue;
                };
                let Some(StoredRow::ToolCall {
                    status,
                    output_text,
                    ..
                }) = transcript.rows.get_mut(index)
                else {
                    continue;
                };
                *status = stored_tool_status(update.get("status").and_then(Value::as_str));
                if let Some(parts) = update.get("content").and_then(Value::as_array) {
                    for part in parts {
                        if let Some(text) = part
                            .get("content")
                            .and_then(|content| content.get("text"))
                            .and_then(Value::as_str)
                        {
                            if !output_text.is_empty() {
                                output_text.push('\n');
                            }
                            output_text.push_str(text);
                        }
                    }
                }
            }
            "plan" => {
                let Some(entries) = update.get("entries").and_then(Value::as_array) else {
                    continue;
                };
                transcript.plan = Some(
                    entries
                        .iter()
                        .filter_map(|entry| {
                            let content = entry.get("content").and_then(Value::as_str)?;
                            Some(StoredPlanEntry {
                                content: content.to_owned(),
                                status: stored_plan_status(
                                    entry.get("status").and_then(Value::as_str).unwrap_or(""),
                                ),
                            })
                        })
                        .collect(),
                );
            }
            "turn_completed" => {
                let Some(usage) = update.get("usage") else {
                    continue;
                };
                transcript.usage = Some(StoredUsage {
                    input_tokens: usage
                        .get("inputTokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    output_tokens: usage
                        .get("outputTokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                });
            }
            // task_backgrounded/task_completed/auto_compact_*/compaction_checkpoint
            // 等其余种类：还原面板这一版不渲染，登记在合同文档里。
            _ => {}
        }
    }
    transcript
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    const CWD: &str = "/home/dev/projects/demo";
    const SESSION: &str = "01a11392-1bd4-7f82-ab78-df12c9f1028c";
    const SECOND: &str = "01b22403-2ce5-8a93-bc89-e023dae2139d";
    const ENCODED: &str = "%2Fhome%2Fdev%2Fprojects%2Fdemo";

    fn write_session(
        root: &Path,
        encoded: &str,
        id: &str,
        summary: serde_json::Value,
        history: &str,
    ) {
        let dir = root.join(encoded).join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("summary.json"), summary.to_string()).unwrap();
        fs::write(dir.join("chat_history.jsonl"), history).unwrap();
    }

    /// `updates.jsonl` 的一行：终端记录 `session/update` 通知的形状。
    fn update_line(update: serde_json::Value) -> String {
        serde_json::json!({
            "timestamp": 1791329752,
            "method": "session/update",
            "params": {"sessionId": "s", "update": update},
        })
        .to_string()
    }

    /// 还原视图要覆盖终端真实写下的形状（本机 updates.jsonl 实测词表）：
    /// 文本块合并、工具卡带终态、计划带状态、每轮真实用量。
    #[test]
    fn updates_transcript_restores_tool_cards_plan_and_usage() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("bucket").join("s1");
        fs::create_dir_all(&dir).unwrap();
        let lines = vec![
            update_line(serde_json::json!({
                "sessionUpdate": "user_message_chunk",
                "content": {"type": "text", "text": "帮我看下端口"},
            })),
            update_line(serde_json::json!({
                "sessionUpdate": "agent_thought_chunk",
                "content": {"type": "text", "text": "用户在问端口"},
            })),
            update_line(serde_json::json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": "我先查一下"},
            })),
            update_line(serde_json::json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": "监听状态。"},
            })),
            update_line(serde_json::json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_1",
                "title": "run_terminal_command",
                "rawInput": {"command": "ss -ltn"},
            })),
            update_line(serde_json::json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_1",
                "status": "completed",
                "content": [{"type": "content", "content": {"type": "text", "text": "LISTEN 8787"}}],
            })),
            update_line(serde_json::json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_2",
                "title": "write_file",
                "rawInput": {"path": "x"},
            })),
            update_line(serde_json::json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_2",
                "status": "failed",
            })),
            update_line(serde_json::json!({
                "sessionUpdate": "plan",
                "entries": [
                    {"content": "查端口", "priority": "medium", "status": "completed"},
                    {"content": "修配置", "priority": "medium", "status": "in_progress"},
                    {"content": "验证", "priority": "medium", "status": "pending"},
                ],
            })),
            update_line(serde_json::json!({
                "sessionUpdate": "turn_completed",
                "usage": {"inputTokens": 18160, "outputTokens": 357},
            })),
        ];
        fs::write(dir.join("updates.jsonl"), lines.join("\n")).unwrap();

        let transcript = read_updates_transcript(&dir);
        assert_eq!(
            transcript.rows.len(),
            5,
            "user + reasoning + assistant + 两张工具卡"
        );
        assert_eq!(
            transcript.rows[0],
            StoredRow::UserInput {
                text: "帮我看下端口".into()
            }
        );
        assert_eq!(
            transcript.rows[1],
            StoredRow::Reasoning {
                text: "用户在问端口".into()
            }
        );
        // 相邻的同种文本块合并成一行。
        assert_eq!(
            transcript.rows[2],
            StoredRow::AssistantText {
                text: "我先查一下监听状态。".into()
            }
        );
        assert_eq!(
            transcript.rows[3],
            StoredRow::ToolCall {
                tool_call_id: "call_1".into(),
                tool_name: "run_terminal_command".into(),
                status: "success",
                input_text: serde_json::json!({"command": "ss -ltn"}).to_string(),
                output_text: "LISTEN 8787".into(),
            }
        );
        assert_eq!(
            transcript.rows[4],
            StoredRow::ToolCall {
                tool_call_id: "call_2".into(),
                tool_name: "write_file".into(),
                status: "error",
                input_text: serde_json::json!({"path": "x"}).to_string(),
                output_text: String::new(),
            }
        );
        let plan = transcript.plan.expect("计划被还原");
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].status, "completed");
        // 终端写的是 snake_case `in_progress`，官方合同是 camelCase `inProgress`。
        assert_eq!(plan[1].status, "inProgress");
        assert_eq!(plan[2].status, "pending");
        let usage = transcript.usage.expect("真实账目");
        assert_eq!(
            usage,
            StoredUsage {
                input_tokens: 18160,
                output_tokens: 357
            }
        );
    }

    /// 没有 `updates.jsonl`（旧格式会话或纯草稿）是常态而非错误：空还原视图，
    /// 调用方回退到文字缓存。
    #[test]
    fn updates_transcript_missing_file_is_an_empty_view() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("bucket").join("s2");
        fs::create_dir_all(&dir).unwrap();
        let transcript = read_updates_transcript(&dir);
        assert_eq!(transcript, StoredTranscript::default());
    }

    const HISTORY: &str = concat!(
        "{\"type\":\"system\",\"content\":\"You are Chaos\"}\n",
        "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"<user_info>\\nOS: linux\\n</user_info>\"}]}\n",
        "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"<user_query>Why is the port closed?</user_query>\"}]}\n",
        "{\"type\":\"assistant\",\"content\":\"Nothing is listening.\",\"tool_calls\":[]}\n",
        "{\"type\":\"tool_result\",\"tool_call_id\":\"t1\",\"content\":\"ok\"}\n",
        "{\"type\":\"reasoning\",\"content\":\"hmm\"}\n",
    );

    /// A store as the CLI actually writes one: a bucket named for the encoded
    /// directory, a session named for its UUID, a summary beside the transcript.
    fn fixture(root: &Path) -> PathBuf {
        write_session(
            root,
            ENCODED,
            SESSION,
            json!({
                "info": {"id": SESSION, "cwd": CWD},
                "session_summary": "Restart the web service",
                "created_at": "2026-10-07T01:00:00.000000Z",
                "updated_at": "2026-10-07T01:05:36.343685Z",
                "num_messages": 894,
                "num_chat_messages": 261,
                "current_model_id": "demo/model"
            }),
            HISTORY,
        );
        root.join(ENCODED)
    }

    #[test]
    fn scan_reports_the_conversation_and_its_directory() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        // Strays the real store also contains: an index file and a non-UUID dir.
        fs::write(root.path().join("session_search.sqlite"), b"sqlite").unwrap();
        fs::create_dir_all(root.path().join("%2Fother").join("not-a-session")).unwrap();

        let found = scan_store(root.path(), SCAN_MAX_SESSIONS);
        assert_eq!(found.len(), 1, "only the real session is listed: {found:?}");
        let meta = &found[0];
        assert_eq!(meta.cwd, CWD);
        assert_eq!(meta.native_id, SESSION);
        assert_eq!(
            meta.display_title().as_deref(),
            Some("Restart the web service")
        );
        assert_eq!(meta.message_count, 261);
        assert_eq!(meta.model.as_deref(), Some("demo/model"));
        assert!(meta.created_at_ms > 0);
        assert!(meta.updated_at_ms >= meta.created_at_ms);
    }

    #[test]
    fn missing_store_is_empty_not_an_error() {
        let missing = Path::new("/nonexistent-chaos-store");
        assert!(scan_store(missing, SCAN_MAX_SESSIONS).is_empty());
    }

    #[test]
    fn summary_cwd_wins_over_the_directory_name() {
        let root = tempfile::tempdir().unwrap();
        // The bucket says one directory, the summary says another: the summary is
        // the truth, exactly as the CLI's own relocation code treats it.
        write_session(
            root.path(),
            "%2Fstale%2Fname",
            SESSION,
            json!({
                "info": {"id": SESSION, "cwd": "/moved/here"},
                "session_summary": "被搬过的那段",
                "updated_at": "2026-10-07T01:00:00Z"
            }),
            "",
        );
        let found = scan_store(root.path(), SCAN_MAX_SESSIONS);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].cwd, "/moved/here");
    }

    #[test]
    fn a_hash_encoded_bucket_falls_back_to_its_cwd_sidecar() {
        let root = tempfile::tempdir().unwrap();
        // Past 255 bytes the CLI names the bucket `{slug}-{hash}` and records the
        // directory in a `.cwd` file next to it. A summary with no cwd — an old
        // one — leaves only that file to ask.
        let bucket = root.path().join("demo-0123456789abcdef");
        fs::create_dir_all(bucket.join(SESSION)).unwrap();
        fs::write(bucket.join(".cwd"), format!("{CWD}\n")).unwrap();
        fs::write(
            bucket.join(SESSION).join("summary.json"),
            json!({
                "session_summary": "长路径桶里的那段",
                "updated_at": "2026-10-07T01:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
        let found = scan_store(root.path(), SCAN_MAX_SESSIONS);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].cwd, CWD);
    }

    #[test]
    fn the_title_follows_the_terminal_generated_title_first() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            ENCODED,
            SESSION,
            json!({
                "info": {"id": SESSION, "cwd": CWD},
                "generated_title": "Restart the web service",
                "session_summary": "first prompt line"
            }),
            "",
        );
        let found = scan_store(root.path(), SCAN_MAX_SESSIONS);
        assert_eq!(
            found[0].display_title().as_deref(),
            Some("Restart the web service"),
            "the picker shows the generated title when there is one"
        );
    }

    #[test]
    fn a_subagents_conversation_is_not_listed() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            ENCODED,
            SESSION,
            json!({
                "info": {"id": SESSION, "cwd": CWD},
                "session_kind": "subagent:research",
                "session_summary": "internal worker"
            }),
            "",
        );
        assert!(
            scan_store(root.path(), SCAN_MAX_SESSIONS).is_empty(),
            "the terminal's own picker hides these; so does this"
        );
    }

    #[test]
    fn a_newer_last_active_at_is_the_time_the_list_sorts_by() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            ENCODED,
            SESSION,
            json!({
                "info": {"id": SESSION, "cwd": CWD},
                "session_summary": "按最后活跃排序",
                "updated_at": "2026-10-01T00:00:00Z",
                "last_active_at": "2026-10-09T00:00:00Z"
            }),
            "",
        );
        let found = scan_store(root.path(), SCAN_MAX_SESSIONS);
        let active = chrono::DateTime::parse_from_rfc3339("2026-10-09T00:00:00Z")
            .unwrap()
            .timestamp_millis() as u64;
        assert_eq!(found[0].updated_at_ms, active);
    }

    #[test]
    fn transcript_keeps_human_and_assistant_turns_and_unwraps_the_query() {
        let root = tempfile::tempdir().unwrap();
        let dir = fixture(root.path());
        let transcript = read_transcript(&dir.join(SESSION));
        assert_eq!(
            transcript,
            vec![
                TimelineMessage {
                    role: "user".into(),
                    text: "Why is the port closed?".into()
                },
                TimelineMessage {
                    role: "assistant".into(),
                    text: "Nothing is listening.".into()
                },
            ],
            "system, injected context, tool results and reasoning are not rows"
        );
    }

    #[test]
    fn title_falls_back_to_the_first_human_turn() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            ENCODED,
            SESSION,
            json!({
                "info": {"id": SESSION, "cwd": CWD},
                "session_summary": "",
                "num_chat_messages": 2
            }),
            concat!(
                "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"<user_info>noise</user_info>\"}]}\n",
                "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"<system-reminder>workflows</system-reminder>\"}]}\n",
                "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"<user_query>Back up the apps folder</user_query>\"}]}\n",
            ),
        );
        let found = scan_store(root.path(), SCAN_MAX_SESSIONS);
        assert_eq!(
            found[0].display_title().as_deref(),
            Some("Back up the apps folder")
        );
    }

    #[test]
    fn unreadable_summary_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(ENCODED).join(SESSION);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("summary.json"), "not json").unwrap();
        assert!(scan_store(root.path(), SCAN_MAX_SESSIONS).is_empty());
    }

    #[test]
    fn decoding_round_trips_the_clis_own_encoding() {
        assert_eq!(decode_cwd_dirname(ENCODED).as_deref(), Some(CWD));
        // A `+` is a literal plus under percent-encoding, never a space.
        assert_eq!(
            decode_cwd_dirname("%2Ftmp%2Fa+b").as_deref(),
            Some("/tmp/a+b")
        );
        // Non-UTF-8 after decoding has no sane directory to name.
        assert_eq!(decode_cwd_dirname("%FF%FE"), None);
        assert_eq!(decode_cwd_dirname("%2"), None);
    }

    #[test]
    fn a_long_transcript_keeps_its_recent_turns() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(ENCODED).join(SESSION);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("summary.json"),
            json!({"info": {"cwd": CWD}}).to_string(),
        )
        .unwrap();
        let mut history = String::new();
        for index in 0..400 {
            history.push_str(
                &json!({"type": "user", "content": [{"type": "text", "text": format!("turn {index}")}]})
                    .to_string(),
            );
            history.push('\n');
        }
        fs::write(dir.join("chat_history.jsonl"), history).unwrap();
        let transcript = read_transcript(&dir);
        assert_eq!(transcript.len(), TRANSCRIPT_MAX_MESSAGES);
        assert_eq!(transcript.last().unwrap().text, "turn 399");
        assert_eq!(transcript.first().unwrap().text, "turn 100");
    }

    /// 本机真实存在的形状：`grok-build-plan` 留下的空壳——没有标题、`num_messages: 0`，
    /// 但 `num_chat_messages: 2`（那两条是注入的技能清单与系统提示词）。终端自己的列表
    /// 把这种记录删掉；界面上留着它，就是用户看到的「一排新会话，真正的对话不见踪影」。
    #[test]
    fn a_nameless_empty_record_is_not_a_conversation() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            ENCODED,
            SESSION,
            json!({
                "info": {"id": SESSION, "cwd": CWD},
                "session_summary": "",
                "num_messages": 0,
                "num_chat_messages": 2,
                "agent_name": "grok-build-plan"
            }),
            concat!(
                "{\"type\":\"system\",\"content\":\"You are Chaos\"}\n",
                "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"<system-reminder>技能清单</system-reminder>\"}]}\n",
            ),
        );
        assert!(
            scan_store(root.path(), SCAN_MAX_SESSIONS).is_empty(),
            "终端不列这种记录，界面也不该列"
        );
    }

    /// 但「先改名再说」的空会话终端是保留的：无名才是删除条件，空本身不是。
    #[test]
    fn a_named_empty_conversation_is_still_listed() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            ENCODED,
            SESSION,
            json!({
                "info": {"id": SESSION, "cwd": CWD},
                "generated_title": "先起好名字的那段",
                "num_messages": 0,
                "num_chat_messages": 0
            }),
            "",
        );
        let found = scan_store(root.path(), SCAN_MAX_SESSIONS);
        assert_eq!(found.len(), 1, "有名字的空对话终端会列：{found:?}");
    }

    /// `grok -p` 的一次性运行在终端里自己有一页。界面上列它们已经够吵，更糟的是
    /// 这个宿主续聊用的就是 `-p`——每次续聊都会在原会话旁边多出一条自己的副本。
    #[test]
    fn a_oneshot_headless_run_is_not_a_conversation() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            ENCODED,
            SESSION,
            json!({
                "info": {"id": SESSION, "cwd": CWD},
                "session_summary": "grok -p 跑的那次",
                "session_kind": "headless",
                "num_messages": 4
            }),
            "",
        );
        assert!(scan_store(root.path(), SCAN_MAX_SESSIONS).is_empty());
    }

    /// `hidden` 显式写了就以它为准——被显式放行的子代理会话终端会列。
    #[test]
    fn an_explicitly_unhidden_subagent_is_listed() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            ENCODED,
            SESSION,
            json!({
                "info": {"id": SESSION, "cwd": CWD},
                "session_summary": "被显式放行的子代理",
                "session_kind": "subagent:research",
                "hidden": false
            }),
            "",
        );
        assert_eq!(scan_store(root.path(), SCAN_MAX_SESSIONS).len(), 1);
    }

    /// 排序键是 `last_active_at`，取不到才退到 `updated_at`——不是两者取大。
    /// 改名、换 agent 这类只动元数据的写入会推新 `updated_at` 而不动 `last_active_at`，
    /// 终端刻意不让这种事把会话提前；取大就会让两端排出不一样。
    #[test]
    fn a_metadata_touch_does_not_promote_a_conversation() {
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            ENCODED,
            SESSION,
            json!({
                "info": {"id": SESSION, "cwd": CWD},
                "session_summary": "上周真的聊过",
                "last_active_at": "2026-10-01T00:00:00Z",
                "updated_at": "2026-10-09T00:00:00Z"
            }),
            "",
        );
        write_session(
            root.path(),
            "%2Fother",
            SECOND,
            json!({
                "info": {"id": SECOND, "cwd": "/other"},
                "session_summary": "昨天聊过",
                "last_active_at": "2026-10-05T00:00:00Z",
                "updated_at": "2026-10-05T00:00:00Z"
            }),
            "",
        );
        let found = scan_store(root.path(), SCAN_MAX_SESSIONS);
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[0].title.as_deref(),
            Some("昨天聊过"),
            "只被改过名字的那段不该浮到上面：{found:?}"
        );
    }

    /// 同一个 id 出现在两个桶里 = 歧义，终端两条都不列。界面若两条都列，同一段对话
    /// 会同时挂在两个目录上，用户点哪条就在哪个目录里续——那是同一段对话的两个分身。
    #[test]
    fn an_id_sitting_in_two_buckets_is_listed_by_neither() {
        let root = tempfile::tempdir().unwrap();
        for encoded in ["%2Fone", "%2Ftwo"] {
            write_session(
                root.path(),
                encoded,
                SESSION,
                json!({
                    "info": {"id": SESSION, "cwd": "/somewhere"},
                    "session_summary": "同一个 id"
                }),
                "",
            );
        }
        assert!(
            scan_store(root.path(), SCAN_MAX_SESSIONS).is_empty(),
            "歧义就宁可不列，也不要一份对话两个归属"
        );
    }

    #[test]
    fn newest_first_across_directories() {
        let root = tempfile::tempdir().unwrap();
        for (index, (encoded, id)) in [("%2Fone", SESSION), ("%2Ftwo", SECOND)]
            .into_iter()
            .enumerate()
        {
            write_session(
                root.path(),
                encoded,
                id,
                json!({
                    "info": {"id": SESSION, "cwd": format!("/{}", index)},
                    "session_summary": format!("第 {} 段", index),
                    "updated_at": if index == 0 { "2026-10-01T00:00:00Z" } else { "2026-10-02T00:00:00Z" }
                }),
                "",
            );
        }
        let found = scan_store(root.path(), SCAN_MAX_SESSIONS);
        assert_eq!(found.len(), 2);
        // `/1` is the conversation of 2026-10-02, the later of the two, even
        // though its bucket sorts second on disk.
        assert_eq!(found[0].cwd, "/1", "the newest conversation comes first");
    }
}
