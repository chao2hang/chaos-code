//! Mock language servers used by the LSP tests.
//!
//! Each is a small Python script speaking LSP over stdio, written to a temp dir
//! and spawned like a real server. They exist so the client can be tested
//! against the *shapes* real servers come in — full versus incremental sync,
//! push versus pull diagnostics, save with or without text — without needing
//! any of those servers installed.
//!
//! One rule every script here obeys and none can be forgiven for breaking: LSP
//! framing leaves through `sys.stdout.buffer`, never through a text stream.
//! Python's text mode rewrites every `\n` to `\r\n` on Windows, so a header
//! terminator spelled `"\r\n\r\n"` arrives as `\r\r\n\r\r\n`, which no LSP parser
//! accepts; the client gives up on the response, drops the server's stdin, the
//! server reads EOF and exits 0, and the entire evidence is `service stopped; the
//! process exited with code 0` with nothing on stderr. Every mock in the
//! 2026-10-03 Windows leg failed that way at once while passing on Linux. Real
//! servers write bytes for the same reason `Content-Length` counts bytes: the
//! body is encoded before it is measured.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// An interpreter to run the mock servers with, and the flags it needs before
/// the script's own arguments (`-3` for the Windows launcher).
#[derive(Debug, PartialEq, Eq)]
struct PythonProgram {
    program: String,
    pre_args: Vec<String>,
}

/// Spellings tried in order.
///
/// `python3` is what every Linux and macOS box has and what these fixtures have
/// always named. `py -3` is last because the Windows launcher is the one that
/// still finds an interpreter when neither bare name is on `PATH`.
const PYTHON_CANDIDATES: &[(&str, &[&str])] = &[
    ("python3", &[]),
    ("python", &[]),
    #[cfg(windows)]
    ("py", &["-3"]),
];

/// The interpreter, worked out once per test binary.
///
/// The reason to look rather than to assume: a name on `PATH` is not proof of an
/// interpreter. On Windows `python3` can resolve to a launcher stub that spawns
/// successfully, prints a notice, and exits, which leaves the client past
/// `spawn` and reporting only that the server stopped; the 2026-10-03 Windows CI
/// leg failed that way for 38 tests at once. A `--version` probe does not catch
/// that shape, because answering `--version` is exactly what such a stub is built
/// to do, so the probe here hands the candidate a script and asks for its output.
fn python_program() -> &'static PythonProgram {
    static PYTHON: OnceLock<PythonProgram> = OnceLock::new();
    PYTHON.get_or_init(|| {
        resolve_python(PYTHON_CANDIDATES).unwrap_or_else(|refusals| {
            panic!(
                "the LSP mock servers are Python scripts and none of {:?} runs one: {}",
                PYTHON_CANDIDATES,
                refusals.join("; ")
            )
        })
    })
}

/// The word a probe script prints for its own sake.
const PROBE_MARKER: &str = "chaos-lsp-probe-ok";

/// Prints [`PROBE_MARKER`] and nothing else.
///
/// `-c` rather than a file, because the question is whether this program runs the
/// code it is handed. That is the one thing a launcher stub cannot fake: it knows
/// `--version` by heart and executes nothing else.
const PROBE_SCRIPT: &str = "import sys; sys.stdout.write('chaos-lsp-probe-ok'); sys.stdout.flush()";

/// Ask one candidate to run a script, and say in one clause why it will not do.
fn probe_python(program: &str, pre: &[&str]) -> Result<(), String> {
    let spelled = if pre.is_empty() {
        program.to_owned()
    } else {
        format!("{program} {}", pre.join(" "))
    };
    let output = std::process::Command::new(program)
        .args(pre)
        .arg("-u")
        .arg("-c")
        .arg(PROBE_SCRIPT)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("{spelled} cannot run a script: {e}"))?;
    if !output.status.success() {
        let exit = match output.status.code() {
            Some(code) => format!("exit code {code}"),
            None => "a signal".to_owned(),
        };
        let said = trim_to(String::from_utf8_lossy(&output.stderr).trim(), 200);
        return Err(format!(
            "{spelled} refused a script ({exit}, stderr: {said})"
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains(PROBE_MARKER) {
        // Matched against the whole output: a stub that talks first and runs
        // nothing after would otherwise slip past on a trimmed tail.
        return Err(format!(
            "{spelled} exited 0 on a script without running it (output: {})",
            trim_to(stdout.trim(), 200)
        ));
    }
    Ok(())
}

/// Short enough for one panic message, without cutting a character in half.
fn trim_to(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The first candidate that runs a script, or why each candidate was not one.
fn resolve_python(candidates: &[(&str, &[&str])]) -> Result<PythonProgram, Vec<String>> {
    let mut refusals = Vec::new();
    for (program, pre) in candidates {
        match probe_python(program, pre) {
            Ok(()) => {
                return Ok(PythonProgram {
                    program: (*program).to_owned(),
                    pre_args: (*pre).iter().map(|arg| (*arg).to_owned()).collect(),
                });
            }
            Err(reason) => refusals.push(reason),
        }
    }
    Err(refusals)
}

/// `command` for a mock server's [`super::super::config::LspServerConfig`].
pub(super) fn python_command() -> String {
    python_program().program.clone()
}

/// `args` for a mock server's config: the script, unbuffered.
pub(super) fn python_args(script_path: &Path) -> Vec<String> {
    let program = python_program();
    let mut args = program.pre_args.clone();
    args.push("-u".to_owned());
    args.push(script_path.to_string_lossy().into_owned());
    args
}

const MOCK_LSP_SERVER: &str = r#"
import json, sys

def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line.strip() == b'':
            break
        if b':' in line:
            key, value = line.split(b':', 1)
            headers[key.strip()] = value.strip()
    length = int(headers.get(b'Content-Length', 0))
    if length == 0:
        return None
    return json.loads(sys.stdin.buffer.read(length))

def send_message(msg):
    body = json.dumps(msg).encode('utf-8')
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    sys.stdout.buffer.flush()

def send_diagnostics(uri):
    send_message({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": uri,
            "diagnostics": [
                {
                    "range": {
                        "start": {"line": 0, "character": 5},
                        "end": {"line": 0, "character": 10}
                    },
                    "severity": 1,
                    "source": "mock",
                    "message": "mock error: undeclared variable"
                },
                {
                    "range": {
                        "start": {"line": 2, "character": 0},
                        "end": {"line": 2, "character": 15}
                    },
                    "severity": 2,
                    "source": "mock",
                    "message": "mock warning: unused import"
                }
            ]
        }
    })

while True:
    msg = read_message()
    if msg is None:
        break

    method = msg.get("method")
    msg_id = msg.get("id")

    if method == "initialize":
        send_message({
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {
                "capabilities": {
                    "textDocumentSync": 1,
                    "definitionProvider": True,
                    "referencesProvider": True
                }
            }
        })
    elif method == "initialized":
        pass
    elif method == "textDocument/didOpen":
        uri = msg["params"]["textDocument"]["uri"]
        send_diagnostics(uri)
    elif method == "textDocument/didChange":
        uri = msg["params"]["textDocument"]["uri"]
        send_diagnostics(uri)
    elif method == "textDocument/didSave":
        pass
    elif method == "textDocument/definition":
        uri = msg["params"]["textDocument"]["uri"]
        send_message({
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": [{
                "uri": uri,
                "range": {
                    "start": {"line": 10, "character": 0},
                    "end": {"line": 10, "character": 20}
                }
            }]
        })
    elif method == "textDocument/references":
        uri = msg["params"]["textDocument"]["uri"]
        send_message({
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": [
                {
                    "uri": uri,
                    "range": {
                        "start": {"line": 5, "character": 0},
                        "end": {"line": 5, "character": 10}
                    }
                },
                {
                    "uri": uri,
                    "range": {
                        "start": {"line": 15, "character": 3},
                        "end": {"line": 15, "character": 13}
                    }
                }
            ]
        })
    elif method == "shutdown":
        send_message({"jsonrpc": "2.0", "id": msg_id, "result": None})
    elif method == "exit":
        break
    elif msg_id is not None:
        # Real servers answer requests they do not implement rather than
        # leaving the client hanging.
        send_message({"jsonrpc": "2.0", "id": msg_id,
                      "error": {"code": -32601, "message": "Method not found"}})
"#;

pub(super) fn write_mock_server() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let script_path = dir.path().join("mock_lsp.py");
    std::fs::write(&script_path, MOCK_LSP_SERVER).unwrap();
    (dir, script_path)
}

pub(super) fn write_delayed_diagnostics_server() -> (tempfile::TempDir, PathBuf) {
    const DELAYED_SERVER: &str = r#"
import json, sys, time

def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line.strip() == b'':
            break
        if b':' in line:
            key, value = line.split(b':', 1)
            headers[key.strip()] = value.strip()
    length = int(headers.get(b'Content-Length', 0))
    if length == 0:
        return None
    return json.loads(sys.stdin.buffer.read(length))

def send_message(msg):
    body = json.dumps(msg).encode('utf-8')
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    sys.stdout.buffer.flush()

while True:
    msg = read_message()
    if msg is None:
        break
    method = msg.get("method")
    msg_id = msg.get("id")
    if method == "initialize":
        send_message({
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {"capabilities": {"textDocumentSync": 1}}
        })
    elif method == "initialized":
        pass
    elif method == "textDocument/didOpen":
        time.sleep(1.0)
        send_message({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": msg["params"]["textDocument"]["uri"],
                "diagnostics": [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 5}
                    },
                    "severity": 1,
                    "source": "delayed",
                    "message": "delayed diagnostic after restart"
                }]
            }
        })
    elif method == "shutdown":
        send_message({"jsonrpc": "2.0", "id": msg_id, "result": None})
    elif method == "exit":
        break
"#;
    let dir = tempfile::tempdir().unwrap();
    let script_path = dir.path().join("delayed_lsp.py");
    std::fs::write(&script_path, DELAYED_SERVER).unwrap();
    (dir, script_path)
}

pub(super) fn write_init_failure_server() -> (tempfile::TempDir, PathBuf) {
    write_init_failure_server_n_times(3)
}

pub(super) fn write_slow_init_server(delay_ms: u64) -> (tempfile::TempDir, PathBuf) {
    let script = format!(
        r#"import json, sys, time

def read_message():
    headers = {{}}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line.strip() == b'':
            break
        if b':' in line:
            key, value = line.split(b':', 1)
            headers[key.strip()] = value.strip()
    length = int(headers.get(b'Content-Length', 0))
    if length == 0:
        return None
    return json.loads(sys.stdin.buffer.read(length))

def send_message(msg):
    body = json.dumps(msg).encode('utf-8')
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    sys.stdout.buffer.flush()

while True:
    msg = read_message()
    if msg is None:
        break
    method = msg.get("method")
    msg_id = msg.get("id")
    if method == "initialize":
        time.sleep({delay_ms} / 1000.0)
        send_message({{
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {{"capabilities": {{"textDocumentSync": 1, "definitionProvider": True}}}}
        }})
    elif method == "initialized":
        pass
    elif method == "textDocument/definition":
        uri = msg["params"]["textDocument"]["uri"]
        send_message({{
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": [{{
                "uri": uri,
                "range": {{
                    "start": {{"line": 1, "character": 0}},
                    "end": {{"line": 1, "character": 5}}
                }}
            }}]
        }})
    elif method == "shutdown":
        send_message({{"jsonrpc": "2.0", "id": msg_id, "result": None}})
    elif method == "exit":
        break
"#
    );
    let dir = tempfile::tempdir().unwrap();
    let script_path = dir.path().join("slow_init_lsp.py");
    std::fs::write(&script_path, script).unwrap();
    (dir, script_path)
}

pub(super) fn write_init_failure_server_n_times(
    failures_before_success: usize,
) -> (tempfile::TempDir, PathBuf) {
    let init_error_payload = format!(
        "{{\"code\": -32603, \"message\": \"init failed on purpose after {} failures\"}}",
        failures_before_success
    );
    let init_error_payload = init_error_payload.replace('"', r#"\""#);
    let script = format!(
        r#"import json, os, sys

FAILURES_BEFORE_SUCCESS = {failures_before_success}
COUNTER_FILE = os.environ["INIT_FAILURE_COUNTER_FILE"]
INIT_ERROR = json.loads("{init_error_payload}")

def read_message():
    headers = {{}}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line.strip() == b'':
            break
        if b':' in line:
            key, value = line.split(b':', 1)
            headers[key.strip()] = value.strip()
    length = int(headers.get(b'Content-Length', 0))
    if length == 0:
        return None
    return json.loads(sys.stdin.buffer.read(length))

def send_message(msg):
    body = json.dumps(msg).encode('utf-8')
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    sys.stdout.buffer.flush()

def increment_attempts():
    attempts = 0
    if os.path.exists(COUNTER_FILE):
        with open(COUNTER_FILE, "r", encoding="utf-8") as f:
            content = f.read().strip()
            if content:
                attempts = int(content)
    attempts += 1
    with open(COUNTER_FILE, "w", encoding="utf-8") as f:
        f.write(str(attempts))
    return attempts

while True:
    msg = read_message()
    if msg is None:
        break
    method = msg.get("method")
    msg_id = msg.get("id")
    if method == "initialize":
        attempts = increment_attempts()
        if attempts <= FAILURES_BEFORE_SUCCESS:
            send_message({{"jsonrpc": "2.0", "id": msg_id, "error": INIT_ERROR}})
            break
        send_message({{
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {{"capabilities": {{"textDocumentSync": 1}}}}
        }})
    elif method == "initialized":
        pass
    elif method == "shutdown":
        send_message({{"jsonrpc": "2.0", "id": msg_id, "result": None}})
    elif method == "exit":
        break
"#
    );
    let dir = tempfile::tempdir().unwrap();
    let script_path = dir.path().join("init_fail_lsp.py");
    std::fs::write(&script_path, script).unwrap();
    (dir, script_path)
}

// ── Roslyn-shaped mock servers ──────────────────────────────────────────
//
// These differ only in how they answer `initialize` and what they do with the
// notifications that follow, so they share one framing preamble rather than
// each carrying its own copy of the JSON-RPC plumbing.

/// `read_message` / `send_message` / `publish` — the same for every mock.
const MOCK_PREAMBLE: &str = r#"
import json, os, sys
from urllib.parse import urlparse
from urllib.request import url2pathname

state = {"saves": 0, "pulls": 0}

def local_path(uri):
    # Decoded, never sliced. A Windows document URI is file:///C:/dir/file, and
    # cutting off "file://" leaves /C:/dir/file, which Python opens under the
    # root of the current drive; a sliced Unix URI also keeps its %20.
    return url2pathname(urlparse(uri).path)

def touch_beside(uri, name):
    # Signals a test through the filesystem: the marker lands next to the
    # document the message is about. Dying here rather than carrying on is about
    # what the mock claims, not about the error message: a mock that could not
    # locate its document must not go on answering for it. Measured on 2026-10-04,
    # this stderr line is not what a failing run shows -- ServerStderr only quotes
    # the tail when a startup fails -- so the test's own wait is what reports it.
    directory = os.path.dirname(local_path(uri))
    try:
        open(os.path.join(directory, name), 'w').close()
    except OSError as failure:
        sys.stderr.write("cannot write marker %s beside %s: %s\n" % (name, directory, failure))
        sys.stderr.flush()
        raise

def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line.strip() == b'':
            break
        if b':' in line:
            key, value = line.split(b':', 1)
            headers[key.strip()] = value.strip()
    length = int(headers.get(b'Content-Length', 0))
    if length == 0:
        return None
    return json.loads(sys.stdin.buffer.read(length))

def send_message(msg):
    body = json.dumps(msg).encode('utf-8')
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    sys.stdout.buffer.flush()

def one_diagnostic(message):
    return [{
        "range": {"start": {"line": 0, "character": 0},
                  "end": {"line": 0, "character": 1}},
        "severity": 1,
        "source": "mock",
        "message": message
    }]

def publish(uri, message):
    send_message({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {"uri": uri, "diagnostics": one_diagnostic(message)}
    })

def reply(msg, result):
    send_message({"jsonrpc": "2.0", "id": msg.get("id"), "result": result})

def publish_at(uri, message, version):
    send_message({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {"uri": uri, "diagnostics": one_diagnostic(message), "version": version}
    })

def notify(method, params=None):
    send_message({"jsonrpc": "2.0", "method": method, "params": params})

def ask(method, params, request_id):
    send_message({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})

def serve(capabilities, handle):
    while True:
        msg = read_message()
        if msg is None:
            return
        method = msg.get("method")
        if method == "initialize":
            reply(msg, {"capabilities": capabilities})
        elif method == "shutdown":
            reply(msg, None)
        elif method == "exit":
            return
        else:
            handle(msg, method)
"#;

/// Write a mock server whose behaviour is `body`, on top of [`MOCK_PREAMBLE`].
pub(super) fn write_python_server(file_name: &str, body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let script_path = dir.path().join(file_name);
    std::fs::write(&script_path, format!("{MOCK_PREAMBLE}\n{body}")).unwrap();
    (dir, script_path)
}

/// A server that says why it will not run and exits with a status.
///
/// The shape of a missing or misinstalled binary: the spawn itself succeeds, so
/// nothing is reported until `initialize` goes unanswered.
pub(super) fn write_dying_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "dies_lsp.py",
        r#"
sys.stderr.write("cannot start: no such root\n")
sys.stderr.flush()
sys.exit(3)
"#,
    )
}

/// A server whose answer is written as text instead of bytes: every `\n` in the
/// framing becomes `\r\n` on the way out.
///
/// This is not a hypothetical shape. It is what a Python mock does on its own on
/// Windows, where `sys.stdout` is a text stream and text mode rewrites `\n` as
/// `\r\n`, so the `\r\n\r\n` terminator between headers and body arrives as
/// `\r\r\n\r\r\n`. Every mock in the 2026-10-03 Windows leg failed that way at
/// once while passing on Linux, and the entire report was `service stopped; the
/// process exited with code 0`: the client stopped reading, dropped the server's
/// stdin, and the server read EOF and exited 0 without writing a word to stderr.
/// Reproducing the byte sequence here gives the diagnostic that names it a test
/// on every platform, rather than one more fact that only a Windows run can check.
pub(super) fn write_translated_newline_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "translated_framing_lsp.py",
        r#"
def send_message(msg):
    body = json.dumps(msg).encode('utf-8')
    for chunk in (b"Content-Length: %d\r\n\r\n" % len(body), body):
        sys.stdout.buffer.write(chunk.replace(b'\n', b'\r\n'))
        sys.stdout.buffer.flush()

serve({"textDocumentSync": 1}, lambda msg, method: None)
"#,
    )
}

/// A server killed by a signal rather than exiting on its own, which is how an
/// out-of-memory kill or a `kill -9` looks from the client's side.
#[cfg(unix)]
pub(super) fn write_killed_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "killed_lsp.py",
        r#"
import os, signal
os.kill(os.getpid(), signal.SIGKILL)
"#,
    )
}

/// A server that declares **incremental** sync (`textDocumentSync: 2`), like
/// Roslyn does. It reports back, as the diagnostic message, whether the
/// `didChange` it received carried a `range`. Roslyn dereferences that range
/// unconditionally and tears its request queue down when it is missing, so a
/// rangeless change against such a server is a client bug.
pub(super) fn write_incremental_sync_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "incremental_lsp.py",
        r#"
def handle(msg, method):
    if method == "textDocument/didOpen":
        publish(msg["params"]["textDocument"]["uri"], "opened")
    elif method == "textDocument/didChange":
        change = msg["params"]["contentChanges"][0]
        uri = msg["params"]["textDocument"]["uri"]
        if change.get("range") is None:
            publish(uri, "changed without range")
        else:
            r = change["range"]
            publish(uri, "changed with range %d:%d-%d:%d" % (
                r["start"]["line"], r["start"]["character"],
                r["end"]["line"], r["end"]["character"]))

serve({"textDocumentSync": 2}, handle)
"#,
    )
}

/// A Roslyn-shaped server: incremental sync, **no** save support, and
/// diagnostics served by pull only — it never publishes. Its diagnostic message
/// reports what the client actually did, so tests can assert on client
/// behaviour rather than on internal state.
pub(super) fn write_pull_diagnostics_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "pull_lsp.py",
        r#"
def handle(msg, method):
    if method == "textDocument/didSave":
        state["saves"] += 1
    elif method == "textDocument/diagnostic":
        state["pulls"] += 1
        previous = msg["params"].get("previousResultId")
        reply(msg, {
            "kind": "full",
            "resultId": "result-%d" % state["pulls"],
            "items": one_diagnostic("pull #%d saves=%d prev=%s" % (
                state["pulls"], state["saves"], previous))
        })

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": True, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// A pull server that answers honestly: a document is clean unless its name
/// says "broken". Used to check that "no problems" counts as an answer rather
/// than as silence, and that a real problem after a run of clean files is still
/// reported promptly.
pub(super) fn write_selective_pull_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "selective_pull_lsp.py",
        r#"
def handle(msg, method):
    if method == "textDocument/diagnostic":
        state["pulls"] += 1
        uri = msg["params"]["textDocument"]["uri"]
        items = one_diagnostic("pulled problem") if "broken" in uri else []
        reply(msg, {"kind": "full", "resultId": "r-%d" % state["pulls"], "items": items})

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": False, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// A pull server that answers the second pull with an empty report before
/// going back to reporting the problem — the shape Roslyn has when it is asked
/// again before it has finished re-analyzing an edit.
pub(super) fn write_mid_analysis_pull_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "mid_analysis_pull_lsp.py",
        r#"
def handle(msg, method):
    if method == "textDocument/diagnostic":
        state["pulls"] += 1
        items = [] if state["pulls"] == 2 else one_diagnostic("real problem %d" % state["pulls"])
        reply(msg, {"kind": "full", "resultId": "r-%d" % state["pulls"], "items": items})

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": False, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// A pull server that takes its time, and every answer names the revision it
/// was asked about — so an answer to superseded text is recognisable on sight.
///
/// When the first pull arrives it touches [`FIRST_PULL_MARKER`] beside the
/// document, which is the moment a test has to edit the file again if it wants
/// an answer to land for a revision the server has since been sent a
/// replacement for. The signal deliberately goes through the filesystem rather
/// than a `publishDiagnostics`: a push is itself an answer, and would be the
/// newest one, which is exactly the thing under test.
pub(super) fn write_slow_pull_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "slow_pull_lsp.py",
        r#"
import time

state["revisions"] = 0

def handle(msg, method):
    if method in ("textDocument/didOpen", "textDocument/didChange"):
        state["revisions"] += 1
    elif method == "textDocument/diagnostic":
        state["pulls"] += 1
        asked_about = state["revisions"]
        if state["pulls"] == 1:
            touch_beside(msg["params"]["textDocument"]["uri"], "first-pull-started")
        time.sleep(0.3)
        reply(msg, {
            "kind": "full",
            "resultId": "r-%d" % state["pulls"],
            "items": one_diagnostic("pull %d answers revision %d" % (
                state["pulls"], asked_about))
        })

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": False, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// The file [`write_slow_pull_server`] touches once its first pull is in flight.
pub(super) const FIRST_PULL_MARKER: &str = "first-pull-started";

/// The file [`write_stale_clean_pull_server`] touches once its second pull is
/// in flight.
pub(super) const SECOND_PULL_MARKER: &str = "second-pull-started";

/// A pull server whose "the file is clean now" answer arrives late, and which
/// then stands by it when asked again with its own result id.
///
/// The first pull reports a problem. The second answers clean, slowly enough
/// that a test can edit the file again first. From the third on, a client that
/// sends back the clean report's id is told "unchanged" — so a client that
/// remembers an id for an answer it never stored will have the server confirm
/// errors the server does not have.
pub(super) fn write_stale_clean_pull_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "stale_clean_pull_lsp.py",
        r#"
import time

def handle(msg, method):
    if method == "textDocument/diagnostic":
        state["pulls"] += 1
        previous = msg["params"].get("previousResultId")
        if state["pulls"] == 1:
            reply(msg, {"kind": "full", "resultId": "r1",
                        "items": one_diagnostic("the problem")})
        elif state["pulls"] == 2:
            touch_beside(msg["params"]["textDocument"]["uri"], "second-pull-started")
            time.sleep(0.3)
            reply(msg, {"kind": "full", "resultId": "clean", "items": []})
        elif previous == "clean":
            reply(msg, {"kind": "unchanged", "resultId": "clean"})
        else:
            reply(msg, {"kind": "full", "resultId": "clean", "items": []})

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": False, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// A pull server that answers for some documents and simply never replies for
/// others — the shape of a server that is working, and productive, but has
/// nothing to say about one particular file, ever. Documents whose name
/// contains "loud" get an error; the rest get silence.
pub(super) fn write_partially_answering_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "partial_pull_lsp.py",
        r#"
def handle(msg, method):
    if method == "textDocument/diagnostic":
        uri = msg["params"]["textDocument"]["uri"]
        if "loud" not in uri:
            return
        state["pulls"] += 1
        reply(msg, {
            "kind": "full",
            "resultId": "r-%d" % state["pulls"],
            "items": one_diagnostic("loud problem")
        })

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": False, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// A server that accepts everything and never reports a diagnostic.
pub(super) fn write_silent_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "silent_lsp.py",
        r#"
def handle(msg, method):
    pass

serve({"textDocumentSync": 1}, handle)
"#,
    )
}

/// A server that asks for `didSave` **with** the document text, and reports
/// back whether it actually got it.
pub(super) fn write_save_with_text_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "save_text_lsp.py",
        r#"
def handle(msg, method):
    if method == "textDocument/didSave":
        has_text = msg["params"].get("text") is not None
        publish(msg["params"]["textDocument"]["uri"], "saved with text=%s" % has_text)

serve({
    "textDocumentSync": {"openClose": True, "change": 1, "save": {"includeText": True}}
}, handle)
"#,
    )
}

/// A pull server in the shape Roslyn has at session start: it answers before it
/// has loaded the solution, so its first answer is empty, and it says so
/// afterwards with `workspace/projectInitializationComplete`.
pub(super) fn write_loads_late_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "loads_late_lsp.py",
        r#"
def handle(msg, method):
    if method == "textDocument/diagnostic":
        state["pulls"] += 1
        if state["pulls"] == 1:
            # Still loading. Nothing to report — yet.
            reply(msg, {"kind": "full", "resultId": "r-1", "items": []})
            notify("workspace/projectInitializationComplete", None)
        else:
            reply(msg, {
                "kind": "full",
                "resultId": "r-%d" % state["pulls"],
                "items": one_diagnostic("found once the solution was loaded")
            })

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": True, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// The same, but announced the way the specification provides for: a
/// `workspace/diagnostic/refresh` request, which the client has to answer.
/// Whether the client answered is reported as the diagnostic message, so a
/// client that advertises `refreshSupport` and then ignores the request fails
/// the test rather than merely logging.
pub(super) fn write_diagnostic_refresh_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "diagnostic_refresh_lsp.py",
        r#"
state["answered_refresh"] = False

def handle(msg, method):
    if method is None and msg.get("id") == 9001:
        state["answered_refresh"] = True
    elif method == "textDocument/diagnostic":
        state["pulls"] += 1
        if state["pulls"] == 1:
            reply(msg, {"kind": "full", "resultId": "r-1", "items": []})
            ask("workspace/diagnostic/refresh", None, 9001)
        else:
            reply(msg, {
                "kind": "full",
                "resultId": "r-%d" % state["pulls"],
                "items": one_diagnostic(
                    "refresh answered=%s" % state["answered_refresh"])
            })

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": True, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// A push server that names the revision it analyzed, and runs one behind: the
/// report for an edit describes the text before it, and the real verdict
/// follows. Servers that fill in `version` let us tell those apart exactly
/// instead of crediting whatever arrives.
pub(super) fn write_versioned_push_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "versioned_push_lsp.py",
        r#"
def handle(msg, method):
    if method == "textDocument/didOpen":
        uri = msg["params"]["textDocument"]["uri"]
        version = msg["params"]["textDocument"]["version"]
        publish_at(uri, "verdict on version %d" % version, version)
    elif method == "textDocument/didChange":
        uri = msg["params"]["textDocument"]["uri"]
        version = msg["params"]["textDocument"]["version"]
        # One revision behind: this describes the text before the edit.
        publish_at(uri, "stale verdict on version %d" % (version - 1), version - 1)

serve({"textDocumentSync": {"openClose": True, "change": 1}}, handle)
"#,
    )
}

/// rust-analyzer's shape: it publishes, *and* it answers
/// `textDocument/diagnostic` — but deliberately with a different, smaller set.
/// Its `cargo check` results only ever arrive by push, so a client that takes
/// the pull answer as the whole picture loses every one of them.
pub(super) fn write_push_and_pull_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "push_and_pull_lsp.py",
        r#"
def handle(msg, method):
    if method in ("textDocument/didOpen", "textDocument/didChange"):
        uri = msg["params"]["textDocument"]["uri"]
        version = msg["params"]["textDocument"]["version"]
        # Toolchain-enabled CI can run many crate tests concurrently; allow the
        # Rust analysis fixture extra time without changing what it publishes.
        import time
        time.sleep(0.05)
        publish_at(uri, "the check that only the push channel runs, pulls=%d" % state["pulls"], version)
    elif method == "textDocument/diagnostic":
        state["pulls"] += 1
        # Answers, and has nothing of its own to say about this file.
        reply(msg, {"kind": "full", "resultId": "r-%d" % state["pulls"], "items": []})

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": True, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// A server that publishes for a file before it has ever been told about it —
/// the shape of a workspace-wide or `cargo check` report arriving for a file
/// the client has not opened.
pub(super) fn write_publishes_before_open_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "publishes_before_open_lsp.py",
        r#"
import os

def handle(msg, method):
    if method == "initialized":
        # Report on a file the client has not opened, the way a workspace-wide
        # or check-on-save pass does.
        publish(os.environ["PREOPENED_URI"], "reported before the file was opened")

serve({"textDocumentSync": {"openClose": True, "change": 1}}, handle)
"#,
    )
}

/// A push-only server that asks for a diagnostics refresh anyway. There is
/// nothing to re-pull from it, so the right response is to leave what it has
/// already told us alone rather than throw it away.
pub(super) fn write_refresh_without_pull_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "refresh_without_pull_lsp.py",
        r#"
def handle(msg, method):
    if method in ("textDocument/didOpen", "textDocument/didChange"):
        uri = msg["params"]["textDocument"]["uri"]
        publish(uri, "a real problem")
        ask("workspace/diagnostic/refresh", None, 9002)

serve({"textDocumentSync": {"openClose": True, "change": 1}}, handle)
"#,
    )
}

/// A server that says nothing of its own accord and does not implement pull
/// diagnostics either. It is asked once, says so, and must not be asked again.
pub(super) fn write_pull_rejecting_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "pull_rejecting_lsp.py",
        r#"
import os

# The count goes to a file, not a diagnostic: a server that publishes is not
# one we pull from, so publishing here would remove the thing being counted.
counted = os.path.join(os.path.dirname(sys.argv[0]), "pulls.txt")

def handle(msg, method):
    if method == "textDocument/diagnostic":
        state["pulls"] += 1
        with open(counted, "w") as f:
            f.write(str(state["pulls"]))
        send_message({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "error": {"code": -32601, "message": "method not found"}
        })

serve({"textDocumentSync": {"openClose": True, "change": 1}}, handle)
"#,
    )
}

/// Reports a real problem once, then answers "clean" twice, then stops
/// answering at all. Enough rope to hang a client that lets a clean answer
/// about replaced text erase what it holds: the two clean answers belong to a
/// revision that has been superseded by the time the second arrives, and the
/// silence afterwards means nothing can quietly put the error back.
pub(super) fn write_clean_then_silent_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "clean_then_silent_lsp.py",
        r#"
def handle(msg, method):
    if method == "textDocument/diagnostic":
        state["pulls"] += 1
        if state["pulls"] == 1:
            items = one_diagnostic("the real problem")
        elif state["pulls"] <= 3:
            items = []
        else:
            return  # no reply at all
        reply(msg, {"kind": "full", "resultId": "r-%d" % state["pulls"], "items": items})

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": True, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// Roslyn's worst-case shape: asked for diagnostics before it has loaded the
/// solution, it does not answer at all. Some time later it announces it is
/// ready, and only then does it start answering — and even then not instantly.
///
/// By the time it speaks, a client that judges silence by the clock has already
/// stopped waiting for it, which is exactly when it must start again.
pub(super) fn write_loads_after_going_quiet_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "loads_after_quiet_lsp.py",
        r#"
import time

def handle(msg, method):
    if method == "textDocument/diagnostic":
        state["pulls"] += 1
        if state["pulls"] == 1:
            # Still loading, and it will not answer questions about code it has
            # not read. Not MethodNotFound — it implements this, it just cannot
            # answer yet.
            send_message({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "error": {"code": -32603, "message": "still loading"}
            })
            # Some time later — long enough that a client watching the clock
            # has given up on it — the solution is open. The caller's patience
            # is 30 ms, so this silence is the observable window it waits for.
            time.sleep(0.4)
            notify("workspace/projectInitializationComplete", None)
            return
        time.sleep(0.25)
        reply(msg, {
            "kind": "full",
            "resultId": "r-%d" % state["pulls"],
            "items": one_diagnostic("found once the solution was loaded")
        })

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": True, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// rust-analyzer at its most dangerous: it answers a pull promptly and has
/// nothing of its own to say, while the errors that matter — the ones only
/// `cargo check` finds — arrive on the push channel a moment later.
///
/// A client that takes the pull answer as the verdict settles the file as
/// clean, and by the time the real errors land nobody is waiting for them.
pub(super) fn write_slow_check_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "slow_check_lsp.py",
        r#"
import threading

def publish_later(uri, version):
    def run():
        import time
        time.sleep(0.3)
        publish_at(uri, "an error only the check finds", version)
    threading.Thread(target=run, daemon=True).start()

def handle(msg, method):
    if method in ("textDocument/didOpen", "textDocument/didChange"):
        publish_later(msg["params"]["textDocument"]["uri"],
                      msg["params"]["textDocument"]["version"])
    elif method == "textDocument/diagnostic":
        state["pulls"] += 1
        # Answers at once, with only what its own analysis knows: nothing.
        reply(msg, {"kind": "full", "resultId": "r-%d" % state["pulls"], "items": []})

serve({
    "textDocumentSync": {"openClose": True, "change": 2},
    "diagnosticProvider": {"interFileDependencies": True, "workspaceDiagnostics": False}
}, handle)
"#,
    )
}

/// A server that behaves like Roslyn on file watching: if the client advertised
/// `didChangeWatchedFiles`, it registers a NuGet-cache glob (the registration
/// that would otherwise become tens of thousands of inotify watches) and
/// records whether the client accepted it. It also records any
/// `workspace/didChangeWatchedFiles` the client later sends.
pub(super) fn write_file_watch_server() -> (tempfile::TempDir, PathBuf) {
    write_python_server(
        "file_watch_lsp.py",
        r#"
import os
HERE = os.path.dirname(os.path.abspath(__file__))

def dump(name, obj):
    # A test polls for the file and parses whatever it finds, so a dump must appear
    # whole: writing the real name first would let a reader see the empty file that
    # `open(..., "w")` leaves behind, which on Windows arrives as a JSON parse error.
    path = os.path.join(HERE, name)
    with open(path + ".part", "w") as f:
        json.dump(obj, f)
    os.replace(path + ".part", path)

while True:
    msg = read_message()
    if msg is None:
        break
    method = msg.get("method")
    if method == "initialize":
        dump("initialize_caps.json", msg["params"]["capabilities"])
        reply(msg, {"capabilities": {"textDocumentSync": 1}})
    elif method == "initialized":
        ask("client/registerCapability", {
            "registrations": [{
                "id": "nuget-dlls",
                "method": "workspace/didChangeWatchedFiles",
                "registerOptions": {
                    "watchers": [
                        {
                            "globPattern": {
                                "baseUri": "file:///tmp/fake-nuget/packages",
                                "pattern": "**/*.dll"
                            }
                        },
                        {
                            "globPattern": "**/*.{ts,tsx,js}"
                        }
                    ]
                }
            }]
        }, 100)
    elif method == "workspace/didChangeWatchedFiles":
        dump("watched.json", msg["params"])
    elif method in ("textDocument/didOpen", "textDocument/didChange", "textDocument/didSave"):
        pass
    elif method == "shutdown":
        reply(msg, None)
    elif method == "exit":
        break
    elif method is None and "id" in msg:
        dump("register_reply.json", msg)
"#,
    )
}

#[cfg(test)]
mod interpreter_tests {
    use super::*;

    /// The notice the Windows Store alias prints in place of an interpreter.
    const ALIAS_NOTICE: &str =
        "Python was not found; run without arguments to install from the Microsoft Store";

    /// The host interpreter spelled as a candidate that runs nothing.
    ///
    /// Python takes the first `-c` and treats everything after it as `sys.argv`,
    /// so code placed here runs and the probe's own script is never reached. That
    /// is the alias's shape, and building the stub from the interpreter the host
    /// already proved is what lets these tests run on Windows, where the alias
    /// lives: a `chmod 755` shell script with a `#!` line could only ever have
    /// run somewhere else.
    fn alias_args<'a>(body: &'a str) -> Vec<&'a str> {
        let mut pre: Vec<&'a str> = python_program()
            .pre_args
            .iter()
            .map(|arg| arg.as_str())
            .collect();
        pre.push("-c");
        pre.push(body);
        pre
    }

    /// The host interpreter as a candidate, which is the one that does run the
    /// script it is handed.
    fn real_args<'a>() -> Vec<&'a str> {
        python_program()
            .pre_args
            .iter()
            .map(|arg| arg.as_str())
            .collect()
    }

    /// A stub whose output comes out of `message_file` rather than out of its own
    /// command line.
    ///
    /// This matters for what the tests can prove. A refusal quotes the candidate's
    /// command line, so a message written into the body is in the refusal no
    /// matter what the probe did with the process; asserting that the refusal
    /// repeats such a message says nothing about the probe. Read from a file, the
    /// message can only reach the refusal by way of what the stub printed.
    fn stub_that_echoes(message_file: &Path, exit_code: i32) -> String {
        let path = message_file.to_string_lossy().into_owned();
        if exit_code == 0 {
            format!("import sys; sys.stdout.write(open({path:?}).read())")
        } else {
            format!("import sys; sys.stderr.write(open({path:?}).read()); sys.exit({exit_code})")
        }
    }

    /// `text` in a file named `name`, for [`stub_that_echoes`]. Named without any
    /// substring the assertions look for, so a path in the quoted command line
    /// cannot satisfy them either.
    fn message_file(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, text).expect("the stub's message has to be readable");
        path
    }

    /// Runs the shipped resolver over candidates whose argument lists are owned.
    fn resolve(candidates: &[(&str, Vec<&str>)]) -> Result<PythonProgram, Vec<String>> {
        let spelled: Vec<(&str, &[&str])> = candidates
            .iter()
            .map(|(program, pre)| (*program, pre.as_slice()))
            .collect();
        resolve_python(&spelled)
    }

    /// The Windows Store alias is exactly this shape: it spawns, complains, and
    /// leaves. Nothing downstream can tell that apart from a real interpreter
    /// unless the probe looks at the exit status.
    #[test]
    fn a_stub_that_refuses_to_run_is_not_an_interpreter() {
        let program = python_program().program.as_str();
        let body = format!("import sys; sys.stderr.write({ALIAS_NOTICE:?}); sys.exit(9009)");
        let resolved = resolve(&[(program, alias_args(&body)), (program, real_args())])
            .expect("the host runs at least one interpreter that executes a script");
        assert_eq!(
            resolved.pre_args,
            python_program().pre_args,
            "the candidate that only prints must lose to the one that runs the script: {:?}",
            resolved
        );
    }

    /// The shape a `--version` probe waves through: answers `--version`, then runs
    /// nothing. That is what the Microsoft Store alias does, and it is why the
    /// probe hands the candidate a script instead of asking it what version it is.
    /// The premise is checked rather than narrated: the stub is asked its version
    /// first, and a `--version` probe really would have taken it.
    #[test]
    fn a_stub_that_answers_version_but_runs_nothing_is_not_an_interpreter() {
        let dir = tempfile::tempdir().unwrap();
        let notice = message_file(&dir, "a.txt", ALIAS_NOTICE);
        let program = python_program().program.as_str();
        let body = format!(
            "import sys; sys.stdout.write('Python 3.12.10' if '--version' in sys.argv \
             else open({notice:?}).read())"
        );
        let alias = alias_args(&body);
        let answered = std::process::Command::new(program)
            .args(&alias)
            .arg("--version")
            .output()
            .expect("the stub spawns; that is the whole problem with it");
        assert_eq!(
            String::from_utf8_lossy(&answered.stdout).trim(),
            "Python 3.12.10",
            "the premise is that a version probe gets an answer: {:?}",
            String::from_utf8_lossy(&answered.stdout)
        );
        let refusals = resolve(&[(program, alias)])
            .expect_err("a candidate that runs no script is not an interpreter");
        assert_eq!(refusals.len(), 1, "{refusals:?}");
        assert!(
            refusals[0].contains(program) && refusals[0].contains("without running it"),
            "the refusal has to name the candidate and the check it failed: {refusals:?}"
        );
        assert!(
            refusals[0].contains(ALIAS_NOTICE),
            "what the stub said instead is the actionable half: {refusals:?}"
        );
    }

    /// One refusal per candidate, in candidate order, each carrying what that
    /// candidate printed, because the panic that ends the run is the only report a
    /// runner without these fixtures ever gets.
    #[test]
    fn every_rejected_candidate_is_named_in_the_refusals() {
        let dir = tempfile::tempdir().unwrap();
        let first_said = "candidate-one-refused-this-way";
        let second_said = "candidate-two-printed-and-left";
        let one = message_file(&dir, "a.txt", first_said);
        let two = message_file(&dir, "b.txt", second_said);
        let program = python_program().program.as_str();
        let refusals = resolve(&[
            (program, alias_args(&stub_that_echoes(&one, 1))),
            (program, alias_args(&stub_that_echoes(&two, 0))),
        ])
        .expect_err("neither candidate runs a script");
        assert_eq!(refusals.len(), 2, "{refusals:?}");
        assert!(
            refusals[0].contains(first_said) && !refusals[0].contains(second_said),
            "the first refusal describes the first candidate only: {refusals:?}"
        );
        assert!(
            refusals[1].contains(second_said) && !refusals[1].contains(first_said),
            "the second refusal describes the second candidate only: {refusals:?}"
        );
    }

    #[test]
    fn a_candidate_that_cannot_even_be_spawned_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("not-there");
        let missing_str = missing.to_str().unwrap().to_owned();
        let refusals = resolve(&[(missing_str.as_str(), vec![])])
            .expect_err("nothing on this host is named `not-there`");
        assert!(
            refusals[0].contains("cannot run a script"),
            "a candidate that is not there has to be reported as that: {refusals:?}"
        );
    }

    #[test]
    fn the_script_comes_last_and_unbuffered() {
        let args = python_args(Path::new("/tmp/mock_lsp.py"));
        assert_eq!(args.last().unwrap(), "/tmp/mock_lsp.py");
        let position = args.iter().position(|a| a == "-u").expect("-u is passed");
        assert_eq!(
            position,
            args.len() - 2,
            "-u goes directly before the script"
        );
    }
}
