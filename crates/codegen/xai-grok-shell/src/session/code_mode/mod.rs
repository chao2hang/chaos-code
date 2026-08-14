//! Code Mode — programmatic tool execution via a Rhai script.
//!
//! A `RunCodeTool` invocation submits a Rhai script that can call any
//! tool the active toolset exposes via `tools_call(name, args)`, plus a
//! few pure built-ins (`json_encode`, `json_decode`). All tool calls in
//! one script batch into a single model round-trip, instead of N separate
//! `tool_use`/`tool_result` cycles.
//!
//! ## Architecture
//!
//! Code Mode is **independent** from the workflow engine:
//!
//! - The workflow engine spawns subagents; it has no `ExecuteTool` host
//!   request, so it cannot host Code Mode.
//! - Code Mode is a short, synchronous, foreground task. The script runs
//!   on a blocking thread; each `tools_call(...)` sends an envelope
//!   (request + `oneshot` reply) to the async session, then blocks on the
//!   reply. This is the same host-call shape the workflow engine uses,
//!   and it matches the lifetime of one `tool_use`/`tool_result` exchange.
//!
//! ## Safety
//!
//! - Per-script op count, wall-clock, and tool-call caps.
//! - No module resolver, no `eval`, no ambient I/O of its own: the script
//!   can only reach the outside world through `tools_call`.
//! - Tool calls are answered by the session's normal dispatch, so
//!   permission/auto-mode checks and hooks apply unchanged. Code Mode does
//!   **not** bypass any safety gate.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Maximum number of Rhai operations a single script may execute.
pub const DEFAULT_MAX_OPS: u64 = 5_000;

/// Maximum wall-clock time for one script (compile + run, including
/// every tool round-trip).
pub const DEFAULT_MAX_WALL_TIME: Duration = Duration::from_secs(30);

/// Maximum number of tool calls a single script may make.
pub const DEFAULT_MAX_TOOL_CALLS: u32 = 32;

/// Maximum size of a single string in the script (16 MiB).
const MAX_SCRIPT_STRING_SIZE: usize = 16 * 1024 * 1024;

/// One `tools_call(name, args)` invocation from a script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    /// Tool name as the script wrote it. The host forwards it to the
    /// active toolset, which matches it by `client_name` — the exact
    /// name the model sees in its tool list. Wrong names come back as
    /// a normal tool error and are catchable with `try { ... }`.
    pub name: String,
    /// Arguments as a JSON object, passed through unchanged.
    pub args: serde_json::Value,
}

/// The host's answer to a [`ToolCallRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    /// `true` when dispatch returned Ok.
    pub ok: bool,
    /// On success, the tool's JSON output value.
    pub value: serde_json::Value,
    /// On failure, a human-readable error.
    pub error: Option<String>,
}

impl ToolCallResult {
    /// A successful result carrying `value`.
    pub fn success(value: serde_json::Value) -> Self {
        Self {
            ok: true,
            value,
            error: None,
        }
    }

    /// A failed result carrying `message`.
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            value: serde_json::Value::Null,
            error: Some(message.into()),
        }
    }
}

/// A tool call plus the channel the host answers it on.
///
/// The host receives these on the [`RunCodeRequest::tool_calls`] channel
/// and must send exactly one [`ToolCallResult`] per envelope. Dropping
/// `reply` without sending aborts the script with an error.
#[derive(Debug)]
pub struct ToolCallEnvelope {
    pub call: ToolCallRequest,
    pub reply: oneshot::Sender<ToolCallResult>,
}

/// Outcome of a Code Mode script run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodeModeOutcome {
    /// The script ran to completion. `result` is the value of its final
    /// expression (conventionally a `complete(value)` call).
    Completed {
        result: serde_json::Value,
        /// How many `tools_call(...)` invocations the script made.
        tool_calls: u32,
    },
    /// The script failed to compile, errored at runtime, hit a budget, or
    /// was cancelled.
    Failed { error: String, tool_calls: u32 },
}

/// Request to execute a Code Mode script.
pub struct RunCodeRequest {
    /// The Rhai source to execute.
    pub script: String,
    /// Bound to the script's `args` global. `Null` if not provided.
    pub args: serde_json::Value,
    /// Per-run op cap; `None` uses [`DEFAULT_MAX_OPS`].
    pub max_ops: Option<u64>,
    /// Per-run wall-clock cap; `None` uses [`DEFAULT_MAX_WALL_TIME`].
    pub max_wall_time: Option<Duration>,
    /// Per-run tool-call cap; `None` uses [`DEFAULT_MAX_TOOL_CALLS`].
    pub max_tool_calls: Option<u32>,
    /// Cancellation signal — the script aborts at its next progress check.
    pub cancel: CancellationToken,
    /// Channel the script's tool calls go out on.
    pub tool_calls: tokio_mpsc::UnboundedSender<ToolCallEnvelope>,
}

impl RunCodeRequest {
    /// Run the script on the current thread, blocking until it finishes.
    ///
    /// Call this from a blocking context (e.g. `spawn_blocking`), never on
    /// an async runtime thread: the script blocks the thread while waiting
    /// for each tool result.
    pub fn run(self) -> CodeModeOutcome {
        let Self {
            script,
            args,
            max_ops,
            max_wall_time,
            max_tool_calls,
            cancel,
            tool_calls,
        } = self;

        let start = Instant::now();
        let max_ops = max_ops.unwrap_or(DEFAULT_MAX_OPS);
        let max_wall = max_wall_time.unwrap_or(DEFAULT_MAX_WALL_TIME);
        let max_tool_calls = max_tool_calls.unwrap_or(DEFAULT_MAX_TOOL_CALLS);

        let mut engine = rhai::Engine::new();
        engine.set_max_operations(max_ops);
        engine.set_max_call_levels(64);
        engine.set_max_expr_depths(128, 64);
        engine.set_max_string_size(MAX_SCRIPT_STRING_SIZE);
        engine.set_max_array_size(65_536);
        engine.set_max_map_size(65_536);
        engine.set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new());
        engine.disable_symbol("eval");

        // Abort on cancellation or wall-clock overrun at the next progress check.
        let progress_cancel = cancel.clone();
        engine.on_progress(move |_ops| {
            if progress_cancel.is_cancelled() || start.elapsed() > max_wall {
                Some(rhai::Dynamic::UNIT)
            } else {
                None
            }
        });

        let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
        register_host_fns(&mut engine, tool_calls, cancel, counter.clone(), max_tool_calls);

        let mut scope = rhai::Scope::new();
        let args_dyn = match rhai::serde::to_dynamic(&args) {
            Ok(d) => d,
            Err(e) => {
                return CodeModeOutcome::Failed {
                    error: format!("invalid args JSON: {e}"),
                    tool_calls: 0,
                };
            }
        };
        scope.push_dynamic("args", args_dyn);

        let ast = match engine.compile(&script) {
            Ok(ast) => ast,
            Err(e) => {
                return CodeModeOutcome::Failed {
                    error: format!("script failed to compile: {e}"),
                    tool_calls: 0,
                };
            }
        };

        let result = engine.eval_ast_with_scope::<rhai::Dynamic>(&mut scope, &ast);
        let made = counter.get();
        match result {
            Ok(value) => CodeModeOutcome::Completed {
                result: dynamic_to_value(value),
                tool_calls: made,
            },
            Err(err) => CodeModeOutcome::Failed {
                error: err.to_string(),
                tool_calls: made,
            },
        }
    }
}

/// Register the Code Mode host functions on a fresh engine.
fn register_host_fns(
    engine: &mut rhai::Engine,
    tool_calls: tokio_mpsc::UnboundedSender<ToolCallEnvelope>,
    cancel: CancellationToken,
    counter: std::rc::Rc<std::cell::Cell<u32>>,
    max_tool_calls: u32,
) {
    // `tools_call(name, args)` — dispatch one tool through the host.
    //
    // Tool names are not enumerated at registration time: the host
    // resolves them against the live toolset, so a script can reach
    // whatever the session actually has, and an unknown name comes back
    // as an ordinary catchable error.
    engine.register_fn(
        "tools_call",
        move |name: &str, args: rhai::Map| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            if cancel.is_cancelled() {
                return Err(runtime_error("cancelled".to_string()));
            }

            let next = counter.get() + 1;
            if next > max_tool_calls {
                return Err(runtime_error(format!(
                    "tool-call cap exceeded: this script may make at most {max_tool_calls} \
                     tool calls"
                )));
            }
            counter.set(next);

            let args = args
                .into_iter()
                .map(|(k, v)| (k.to_string(), dynamic_to_value(v)))
                .collect::<serde_json::Map<String, serde_json::Value>>();

            let (reply_tx, reply_rx) = oneshot::channel();
            tool_calls
                .send(ToolCallEnvelope {
                    call: ToolCallRequest {
                        name: name.to_string(),
                        args: serde_json::Value::Object(args),
                    },
                    reply: reply_tx,
                })
                .map_err(|_| {
                    runtime_error(
                        "code-mode host channel closed before the tool call was delivered"
                            .to_string(),
                    )
                })?;

            let result = reply_rx.blocking_recv().map_err(|_| {
                runtime_error("code-mode host dropped the tool result".to_string())
            })?;

            if result.ok {
                Ok(value_to_dynamic(&result.value))
            } else {
                Err(runtime_error(result.error.unwrap_or_else(|| {
                    "tool call failed without a message".to_string()
                })))
            }
        },
    );

    // `complete(value)` is the conventional way to end a script. It is
    // the identity function: the script's final expression is its result,
    // so `complete(x)` simply makes that intent explicit and readable.
    engine.register_fn("complete", |value: rhai::Dynamic| -> rhai::Dynamic { value });
    engine.register_fn("complete", || -> rhai::Dynamic { rhai::Dynamic::UNIT });

    engine.register_fn(
        "json_encode",
        |value: rhai::Dynamic| -> Result<String, Box<rhai::EvalAltResult>> {
            serde_json::to_string(&dynamic_to_value(value))
                .map_err(|e| runtime_error(format!("json_encode failed: {e}")))
        },
    );
    engine.register_fn(
        "json_decode",
        |text: &str| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            let value: serde_json::Value = serde_json::from_str(text)
                .map_err(|e| runtime_error(format!("json_decode failed: {e}")))?;
            Ok(value_to_dynamic(&value))
        },
    );
}

fn runtime_error(message: String) -> Box<rhai::EvalAltResult> {
    Box::new(rhai::EvalAltResult::ErrorRuntime(
        rhai::Dynamic::from(message),
        rhai::Position::NONE,
    ))
}

fn dynamic_to_value(d: rhai::Dynamic) -> serde_json::Value {
    if d.is_unit() {
        serde_json::Value::Null
    } else if d.is::<bool>() {
        serde_json::Value::Bool(d.cast::<bool>())
    } else if d.is::<rhai::INT>() {
        serde_json::Value::Number(d.cast::<rhai::INT>().into())
    } else if d.is::<f64>() {
        serde_json::Number::from_f64(d.cast::<f64>())
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null)
    } else if d.is::<rhai::Map>() {
        d.cast::<rhai::Map>()
            .into_iter()
            .map(|(k, v)| (k.to_string(), dynamic_to_value(v)))
            .collect::<serde_json::Map<String, serde_json::Value>>()
            .into()
    } else if d.is::<rhai::Array>() {
        d.cast::<rhai::Array>()
            .into_iter()
            .map(dynamic_to_value)
            .collect::<Vec<_>>()
            .into()
    } else {
        // Strings and anything else with a display form.
        serde_json::Value::String(d.to_string())
    }
}

fn value_to_dynamic(v: &serde_json::Value) -> rhai::Dynamic {
    match v {
        serde_json::Value::Null => rhai::Dynamic::UNIT,
        serde_json::Value::Bool(b) => rhai::Dynamic::from(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                rhai::Dynamic::from(i as rhai::INT)
            } else if let Some(f) = n.as_f64() {
                rhai::Dynamic::from(f)
            } else {
                rhai::Dynamic::UNIT
            }
        }
        serde_json::Value::String(s) => rhai::Dynamic::from(s.clone()),
        serde_json::Value::Array(a) => {
            rhai::Dynamic::from(a.iter().map(value_to_dynamic).collect::<rhai::Array>())
        }
        serde_json::Value::Object(o) => {
            let mut map = rhai::Map::new();
            for (k, val) in o {
                map.insert(k.clone().into(), value_to_dynamic(val));
            }
            rhai::Dynamic::from(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `script` against a host that answers each tool call from
    /// `canned`, in order. Returns the outcome plus the calls the host saw.
    fn run_with_host(
        script: &str,
        canned: Vec<ToolCallResult>,
    ) -> (CodeModeOutcome, Vec<ToolCallRequest>) {
        let (tx, mut rx) = tokio_mpsc::unbounded_channel::<ToolCallEnvelope>();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_for_host = seen.clone();

        let host = std::thread::spawn(move || {
            let mut canned = canned.into_iter();
            while let Some(envelope) = rx.blocking_recv() {
                seen_for_host.lock().unwrap().push(envelope.call.clone());
                let answer = canned.next().unwrap_or_else(|| {
                    ToolCallResult::failure("test host ran out of canned results")
                });
                let _ = envelope.reply.send(answer);
            }
        });

        let outcome = RunCodeRequest {
            script: script.to_string(),
            args: serde_json::Value::Null,
            max_ops: None,
            max_wall_time: None,
            max_tool_calls: None,
            cancel: CancellationToken::new(),
            tool_calls: tx,
        }
        .run();

        // Dropping the sender happened inside run(); let the host finish.
        host.join().expect("test host thread panicked");
        let seen = seen.lock().unwrap().clone();
        (outcome, seen)
    }

    #[test]
    fn pure_script_returns_its_final_value() {
        let (outcome, seen) = run_with_host(r#" complete("hello from rhai") "#, vec![]);
        assert_eq!(
            outcome,
            CodeModeOutcome::Completed {
                result: serde_json::json!("hello from rhai"),
                tool_calls: 0,
            }
        );
        assert!(seen.is_empty());
    }

    #[test]
    fn tool_call_round_trips_through_the_host() {
        let (outcome, seen) = run_with_host(
            r#"
            let listing = tools_call("bash", #{ command: "ls -la" });
            complete(listing)
            "#,
            vec![ToolCallResult::success(serde_json::json!("file1\nfile2\n"))],
        );
        assert_eq!(
            outcome,
            CodeModeOutcome::Completed {
                result: serde_json::json!("file1\nfile2\n"),
                tool_calls: 1,
            }
        );
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].name, "bash");
        assert_eq!(seen[0].args["command"], "ls -la");
    }

    #[test]
    fn several_tool_calls_batch_into_one_script_run() {
        // This is the whole point of Code Mode: three tool calls, one
        // model round-trip.
        let (outcome, seen) = run_with_host(
            r#"
            let a = tools_call("read_file", #{ path: "/a" });
            let b = tools_call("read_file", #{ path: "/b" });
            let c = tools_call("read_file", #{ path: "/c" });
            complete(#{ combined: a + b + c })
            "#,
            vec![
                ToolCallResult::success(serde_json::json!("A")),
                ToolCallResult::success(serde_json::json!("B")),
                ToolCallResult::success(serde_json::json!("C")),
            ],
        );
        match outcome {
            CodeModeOutcome::Completed { result, tool_calls } => {
                assert_eq!(tool_calls, 3);
                assert_eq!(result["combined"], "ABC");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn structured_tool_output_is_navigable_in_the_script() {
        // A script can branch on structured output instead of shipping it
        // back to the model — the token saving that motivates Code Mode.
        let (outcome, _) = run_with_host(
            r#"
            let hits = tools_call("grep", #{ pattern: "TODO" });
            let count = hits.matches.len();
            complete(#{ found: count, first: hits.matches[0].path })
            "#,
            vec![ToolCallResult::success(serde_json::json!({
                "matches": [
                    { "path": "src/a.rs", "line": 12 },
                    { "path": "src/b.rs", "line": 40 },
                ]
            }))],
        );
        match outcome {
            CodeModeOutcome::Completed { result, .. } => {
                assert_eq!(result["found"], 2);
                assert_eq!(result["first"], "src/a.rs");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn failed_tool_call_becomes_a_catchable_script_error() {
        let (outcome, _) = run_with_host(
            r#"
            let out = "";
            try {
                tools_call("bash", #{ command: "false" });
                out = "unreachable";
            } catch (e) {
                out = "caught";
            }
            complete(out)
            "#,
            vec![ToolCallResult::failure("exit code 1")],
        );
        assert_eq!(
            outcome,
            CodeModeOutcome::Completed {
                result: serde_json::json!("caught"),
                tool_calls: 1,
            }
        );
    }

    #[test]
    fn uncaught_tool_failure_fails_the_run_with_the_host_message() {
        let (outcome, _) = run_with_host(
            r#" tools_call("bash", #{ command: "false" }) "#,
            vec![ToolCallResult::failure("exit code 1")],
        );
        match outcome {
            CodeModeOutcome::Failed { error, tool_calls } => {
                assert_eq!(tool_calls, 1);
                assert!(error.contains("exit code 1"), "error was: {error}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_cap_stops_a_runaway_loop() {
        let (tx, mut rx) = tokio_mpsc::unbounded_channel::<ToolCallEnvelope>();
        let host = std::thread::spawn(move || {
            let mut answered = 0u32;
            while let Some(envelope) = rx.blocking_recv() {
                answered += 1;
                let _ = envelope
                    .reply
                    .send(ToolCallResult::success(serde_json::json!(answered)));
            }
            answered
        });

        let outcome = RunCodeRequest {
            script: r#"
                for i in 0..100 {
                    tools_call("x", #{ i: i });
                }
                complete("unreachable")
            "#
            .to_string(),
            args: serde_json::Value::Null,
            max_ops: None,
            max_wall_time: None,
            max_tool_calls: Some(3),
            cancel: CancellationToken::new(),
            tool_calls: tx,
        }
        .run();

        let answered = host.join().expect("host thread panicked");
        match outcome {
            CodeModeOutcome::Failed { error, tool_calls } => {
                // The script is allowed 3 calls; the 4th trips the cap
                // before it ever reaches the host.
                assert_eq!(tool_calls, 3, "the 3 allowed calls are the ones that count");
                assert_eq!(answered, 3, "only 3 calls reach the host");
                assert!(error.contains("tool-call cap exceeded"), "error was: {error}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn max_ops_aborts_a_runaway_pure_loop() {
        let (tx, _rx) = tokio_mpsc::unbounded_channel::<ToolCallEnvelope>();
        let outcome = RunCodeRequest {
            script: r#"
                let i = 0;
                loop { i += 1; }
                complete(i)
            "#
            .to_string(),
            args: serde_json::Value::Null,
            max_ops: Some(500),
            max_wall_time: None,
            max_tool_calls: None,
            cancel: CancellationToken::new(),
            tool_calls: tx,
        }
        .run();
        assert!(
            matches!(outcome, CodeModeOutcome::Failed { .. }),
            "an infinite loop must not run forever, got {outcome:?}"
        );
    }

    #[test]
    fn cancellation_aborts_the_script() {
        let (tx, _rx) = tokio_mpsc::unbounded_channel::<ToolCallEnvelope>();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let outcome = RunCodeRequest {
            script: r#"
                let i = 0;
                loop { i += 1; }
                complete(i)
            "#
            .to_string(),
            args: serde_json::Value::Null,
            max_ops: None,
            max_wall_time: None,
            max_tool_calls: None,
            cancel,
            tool_calls: tx,
        }
        .run();
        assert!(
            matches!(outcome, CodeModeOutcome::Failed { .. }),
            "a cancelled run must fail, got {outcome:?}"
        );
    }

    #[test]
    fn args_are_bound_into_the_script_scope() {
        let (tx, _rx) = tokio_mpsc::unbounded_channel::<ToolCallEnvelope>();
        let outcome = RunCodeRequest {
            script: r#" complete(args.target + ":" + args.count) "#.to_string(),
            args: serde_json::json!({ "target": "src", "count": 3 }),
            max_ops: None,
            max_wall_time: None,
            max_tool_calls: None,
            cancel: CancellationToken::new(),
            tool_calls: tx,
        }
        .run();
        assert_eq!(
            outcome,
            CodeModeOutcome::Completed {
                result: serde_json::json!("src:3"),
                tool_calls: 0,
            }
        );
    }

    #[test]
    fn compile_error_is_reported_without_running() {
        let (tx, _rx) = tokio_mpsc::unbounded_channel::<ToolCallEnvelope>();
        let outcome = RunCodeRequest {
            script: "this is not ( valid rhai".to_string(),
            args: serde_json::Value::Null,
            max_ops: None,
            max_wall_time: None,
            max_tool_calls: None,
            cancel: CancellationToken::new(),
            tool_calls: tx,
        }
        .run();
        match outcome {
            CodeModeOutcome::Failed { error, tool_calls } => {
                assert_eq!(tool_calls, 0);
                assert!(error.contains("failed to compile"), "error was: {error}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn json_encode_and_decode_round_trip() {
        let (tx, _rx) = tokio_mpsc::unbounded_channel::<ToolCallEnvelope>();
        let outcome = RunCodeRequest {
            script: r#"
                let original = #{ a: 1, b: "two" };
                let text = json_encode(original);
                let back = json_decode(text);
                complete(back.b)
            "#
            .to_string(),
            args: serde_json::Value::Null,
            max_ops: None,
            max_wall_time: None,
            max_tool_calls: None,
            cancel: CancellationToken::new(),
            tool_calls: tx,
        }
        .run();
        assert_eq!(
            outcome,
            CodeModeOutcome::Completed {
                result: serde_json::json!("two"),
                tool_calls: 0,
            }
        );
    }

    #[test]
    fn scripts_cannot_import_modules() {
        let (tx, _rx) = tokio_mpsc::unbounded_channel::<ToolCallEnvelope>();
        let outcome = RunCodeRequest {
            script: r#" import "std" as std; complete(1) "#.to_string(),
            args: serde_json::Value::Null,
            max_ops: None,
            max_wall_time: None,
            max_tool_calls: None,
            cancel: CancellationToken::new(),
            tool_calls: tx,
        }
        .run();
        assert!(
            matches!(outcome, CodeModeOutcome::Failed { .. }),
            "module imports must be refused, got {outcome:?}"
        );
    }
}
