# Code Mode Safety

> Code Mode (`run_code` tool) lets the model submit a Rhai script that
> batches multiple tool calls into one round-trip. This document describes
> the safety controls that prevent a submitted script from doing anything
> the agent couldn't already do via individual `tool_use` calls.

## Threat model

The threat Code Mode defends against is **amplification**: a single
`run_code` invocation can issue up to 32 tool calls in one round-trip, so
a budget overrun or a sandbox escape would compound faster than N separate
calls. The defense is layered:

1. **Rhai sandbox** — the script language itself is locked down.
2. **Budget caps** — op count, wall-clock, and tool-call count prevent
   runaway scripts.
3. **Permission inheritance** — every `tools_call(...)` goes through the
   same dispatch path as a normal `tool_use`, so permission rules,
   auto-mode checks, and hooks apply unchanged.
4. **Preset visibility** — `run_code` is only exposed in presets that
   already trust the agent with a shell (`code` / `ask`), not in
   read-only presets (`explore` / `plan`).

## Rhai sandbox

The Rhai engine is configured with:

| Setting | Value | Purpose |
|---|---|---|
| `set_max_operations` | 5,000 | Bounds total compute per script |
| `set_max_call_levels` | 64 | Prevents deep recursion |
| `set_max_expr_depths` | 128 (statements) / 64 (expressions) | Prevents parser abuse |
| `set_max_string_size` | 16 MiB | Prevents memory exhaustion |
| `set_max_array_size` | 65,536 | Prevents memory exhaustion |
| `set_max_map_size` | 65,536 | Prevents memory exhaustion |
| `module_resolver` | `DummyModuleResolver` | No file/system module loading |
| `disable_symbol("eval")` | — | No `eval` / dynamic code execution |

The script has **no ambient I/O**: it cannot read files, spawn processes,
or touch the network directly. The only way to reach the outside world is
through `tools_call(name, args)`, which is answered by the session's
normal tool dispatch.

## Budget caps

| Cap | Default | Configurable per-call? |
|---|---|---|
| Max operations | 5,000 | Yes (`max_ops` on `RunCodeRequest`) |
| Wall-clock timeout | 30 seconds | Yes (`max_wall_time`) |
| Max tool calls | 32 | Yes (`max_tool_calls`) |

A script that exceeds any cap is aborted at the next progress check
(`engine.on_progress`), and the tool returns a `Failed` outcome with the
budget that was hit. The progress check also honors a `CancellationToken`,
so a user pressing Ctrl+C aborts the script immediately.

## Permission inheritance

Every `tools_call(name, args)` in a script sends a `ToolCallEnvelope` over
a channel to the async session, which forwards it to
`workspace_ops.call_tool()`. This goes through `ToolBridge` — the same path
as a normal `tool_use` — so:

- **Permission rules** (allow / deny / ask) apply to each call.
- **Auto-mode checks** (plan mode, read-only mode) apply to each call.
- **Hooks** (pre-tool, post-tool) fire for each call.
- **Tool denylist** (`--disallowed-tools`) is honored.

Code Mode does **not** bypass any safety gate. If a tool would be denied
as a standalone `tool_use`, it is denied inside `run_code` too.

## Preset visibility

`run_code` is registered in `ToolRegistryBuilder::new()` (so it appears in
the tool metadata), but it is only added to the `tools` list of presets
that already trust the agent with a shell:

| Preset | Exposes `run_code`? | Rationale |
|---|---|---|
| `grok-build` (code) | ✅ | Full shell access already granted |
| `grok-build-concise` | ✅ | Concise shell variant |
| `grok-build-plan` | ✅ | Plan-mode variant of grok-build |
| `grok-build-ask-user` | ✅ | Ask variant of grok-build |
| `codex` | ❌ | Separate tool ecosystem |
| `explore` | ❌ | Read-only — no shell, no code exec |
| `plan` | ❌ | Read-only — no shell, no code exec |
| `grok-computer` | ❌ | Computer-use toolset |

Users who want `run_code` in `plan` preset can configure it explicitly:

```toml
[preset.plan-extended]
base = "plan"
tools = { run_code = true }
```

## Related

- `crates/codegen/xai-grok-shell/src/session/code_mode/mod.rs` — Rhai runtime
- `crates/codegen/xai-grok-tools/src/implementations/grok_build/run_code/mod.rs` — Tool definition
- `crates/codegen/xai-grok-agent/src/config.rs` — Preset definitions
- `docs/telemetry-policy.md` — Telemetry defaults (separate concern)
