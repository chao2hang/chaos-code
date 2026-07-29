#!/usr/bin/env python3
"""Backfill `sessions/usage.sqlite` from historical `updates.jsonl`.

Chaos before 0.2.122 persisted per-session usage to the aggregate SQLite
store only when the user opened `/usage` or the token overlay. Sessions
where the user just chatted and closed were invisible to the cumulative
view, even though their per-turn spend was recorded to the session's
`updates.jsonl`.

This one-shot script scans every session directory under
`<chaos_home>/sessions/`, picks the last `TurnCompleted` update (which
carries the full ledger snapshot the on-demand handler would have
persisted), and upserts it into `usage.sqlite` using the same schema.

Idempotent — rerunning it produces the same rows because
`session_model_usage` has `PRIMARY KEY (session_id, model)` and this
script issues `INSERT ... ON CONFLICT DO UPDATE`, matching what the
Rust `UsageStore::upsert_session_model_usage` writes at runtime.

Chaos home resolution matches the Rust side:
  $CHAOS_HOME > $GROK_HOME > ~/.chaos (if exists) > ~/.grok (if exists) > ~/.chaos
"""

from __future__ import annotations

import json
import os
import sqlite3
import sys
import time
from pathlib import Path
from typing import Optional


def resolve_chaos_home() -> Path:
    for env_var in ("CHAOS_HOME", "GROK_HOME"):
        v = os.environ.get(env_var)
        if v:
            return Path(v).expanduser()
    for name in (".chaos", ".grok"):
        p = Path.home() / name
        if p.is_dir():
            return p
    return Path.home() / ".chaos"


def read_last_turn_completed_usage(updates_path: Path) -> Optional[dict]:
    """Return the freshest ledger snapshot (`.usage` from the most recent
    `TurnCompleted` update)."""
    last_usage: Optional[dict] = None
    try:
        with updates_path.open("r", encoding="utf-8", errors="replace") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    record = json.loads(line)
                except json.JSONDecodeError:
                    continue
                # SessionUpdate::TurnCompleted { usage, ... }
                usage = _extract_usage(record)
                if usage is not None:
                    last_usage = usage
    except OSError:
        return None
    return last_usage


def _extract_usage(record: object) -> Optional[dict]:
    """Walk a JSONL record and return the first `usage` object shaped like
    a `PromptUsage`. Records use camelCase (`inputTokens`, `modelUsage`)."""
    if not isinstance(record, dict):
        return None
    stack: list[object] = [record]
    while stack:
        node = stack.pop()
        if isinstance(node, dict):
            usage = node.get("usage")
            if _looks_like_prompt_usage(usage):
                return usage  # type: ignore[return-value]
            stack.extend(node.values())
        elif isinstance(node, list):
            stack.extend(node)
    return None


def _looks_like_prompt_usage(u: object) -> bool:
    return (
        isinstance(u, dict)
        and "inputTokens" in u
        and "outputTokens" in u
        and "totalTokens" in u
    )


# Wire-model sentinels that some OpenAI-compatible gateways (notably
# Volcengine Ark's `/api/coding/v3`) echo instead of the requested model id.
# When a bucket is keyed by one of these, we prefer any non-sentinel
# assistant `modelId` seen in the same session — matching the runtime fix
# in `record_model_call_usage`.
_WIRE_SENTINELS = {"auto", "default", ""}


def read_configured_model(updates_path: Path) -> Optional[str]:
    """Scan JSONL for the freshest non-sentinel assistant `modelId`."""
    resolved: Optional[str] = None
    try:
        with updates_path.open("r", encoding="utf-8", errors="replace") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                # Cheap prefilter to skip lines without a modelId field.
                if '"modelId"' not in line and '"model_id"' not in line:
                    continue
                try:
                    record = json.loads(line)
                except json.JSONDecodeError:
                    continue
                mid = _walk_for_model_id(record)
                if mid is not None:
                    resolved = mid
    except OSError:
        return None
    return resolved


def _walk_for_model_id(record: object) -> Optional[str]:
    stack: list[object] = [record]
    found: Optional[str] = None
    while stack:
        node = stack.pop()
        if isinstance(node, dict):
            for key in ("modelId", "model_id"):
                val = node.get(key)
                if isinstance(val, str) and val and val not in _WIRE_SENTINELS:
                    found = val
            stack.extend(node.values())
        elif isinstance(node, list):
            stack.extend(node)
    return found


def relabel_sentinels(
    model_usage: dict, configured: Optional[str]
) -> dict:
    """Rewrite `"auto"`-style bucket keys to the configured model id."""
    if not configured or configured in _WIRE_SENTINELS:
        return model_usage
    out: dict = {}
    for name, m in model_usage.items():
        key = configured if name in _WIRE_SENTINELS else name
        if key in out and isinstance(out[key], dict) and isinstance(m, dict):
            # Fold duplicates (e.g. both `"auto"` and the configured id in
            # the same snapshot) by summing numeric fields.
            for field in (
                "inputTokens",
                "outputTokens",
                "cachedReadTokens",
                "reasoningTokens",
                "modelCalls",
                "apiDurationMs",
            ):
                out[key][field] = int(out[key].get(field, 0)) + int(
                    m.get(field, 0)
                )
            # Sum costs when both present, else keep whichever is set.
            left = out[key].get("costUsdTicks")
            right = m.get("costUsdTicks")
            if left is not None and right is not None:
                out[key]["costUsdTicks"] = int(left) + int(right)
            elif right is not None:
                out[key]["costUsdTicks"] = right
            out[key]["costIsPartial"] = bool(
                out[key].get("costIsPartial") or m.get("costIsPartial")
            )
        else:
            out[key] = m
    return out


def ensure_schema(conn: sqlite3.Connection) -> None:
    """Match the Rust `UsageStore` schema. Safe to run repeatedly."""
    conn.executescript(
        """
        CREATE TABLE IF NOT EXISTS session_model_usage (
            session_id TEXT NOT NULL,
            model TEXT NOT NULL,
            recorded_at INTEGER NOT NULL,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cached_read_tokens INTEGER NOT NULL DEFAULT 0,
            reasoning_tokens INTEGER NOT NULL DEFAULT 0,
            model_calls INTEGER NOT NULL DEFAULT 0,
            api_duration_ms INTEGER NOT NULL DEFAULT 0,
            cost_usd_ticks INTEGER,
            cost_is_partial INTEGER NOT NULL DEFAULT 0,
            usage_is_incomplete INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (session_id, model)
        );
        CREATE INDEX IF NOT EXISTS idx_session_model_usage_model
            ON session_model_usage(model);
        CREATE INDEX IF NOT EXISTS idx_session_model_usage_recorded_at
            ON session_model_usage(recorded_at);
        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        """
    )


UPSERT_SQL = """
INSERT INTO session_model_usage(
    session_id, model, recorded_at,
    input_tokens, output_tokens, cached_read_tokens,
    reasoning_tokens, model_calls, api_duration_ms,
    cost_usd_ticks, cost_is_partial, usage_is_incomplete
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(session_id, model) DO UPDATE SET
    recorded_at = excluded.recorded_at,
    input_tokens = excluded.input_tokens,
    output_tokens = excluded.output_tokens,
    cached_read_tokens = excluded.cached_read_tokens,
    reasoning_tokens = excluded.reasoning_tokens,
    model_calls = excluded.model_calls,
    api_duration_ms = excluded.api_duration_ms,
    cost_usd_ticks = excluded.cost_usd_ticks,
    cost_is_partial = excluded.cost_is_partial,
    usage_is_incomplete = excluded.usage_is_incomplete
"""


def upsert(
    conn: sqlite3.Connection,
    session_id: str,
    model: str,
    recorded_at: int,
    m: dict,
    usage_is_incomplete: bool,
) -> None:
    conn.execute(
        UPSERT_SQL,
        (
            session_id,
            model,
            recorded_at,
            int(m.get("inputTokens", 0)),
            int(m.get("outputTokens", 0)),
            int(m.get("cachedReadTokens", 0)),
            int(m.get("reasoningTokens", 0)),
            int(m.get("modelCalls", 0)),
            int(m.get("apiDurationMs", 0)),
            m.get("costUsdTicks"),
            1 if m.get("costIsPartial") else 0,
            1 if usage_is_incomplete else 0,
        ),
    )


def is_subagent_session(session_dir: Path) -> bool:
    """Heuristic mirror of `session_kind_is_subagent`: subagent sessions
    write a `session_kind` field starting with `subagent`."""
    for name in ("session_info.json", "session.json"):
        p = session_dir / name
        if p.is_file():
            try:
                data = json.loads(p.read_text(encoding="utf-8", errors="replace"))
            except (OSError, json.JSONDecodeError):
                continue
            kind = data.get("session_kind") or data.get("sessionKind")
            if isinstance(kind, str) and kind.startswith("subagent"):
                return True
    return False


def main() -> int:
    chaos_home = resolve_chaos_home()
    sessions_dir = chaos_home / "sessions"
    if not sessions_dir.is_dir():
        print(f"no sessions directory under {chaos_home}", file=sys.stderr)
        return 1

    db_path = sessions_dir / "usage.sqlite"
    dry_run = "--dry-run" in sys.argv or "-n" in sys.argv

    conn = sqlite3.connect(db_path)
    ensure_schema(conn)

    now = int(time.time())
    updated_sessions = 0
    inserted_rows = 0
    skipped_subagent = 0
    scanned = 0

    for updates_path in sessions_dir.glob("**/updates.jsonl"):
        scanned += 1
        session_dir = updates_path.parent
        session_id = session_dir.name

        if is_subagent_session(session_dir):
            skipped_subagent += 1
            continue

        usage = read_last_turn_completed_usage(updates_path)
        if usage is None:
            continue

        totals = usage
        model_usage = usage.get("modelUsage") or {}
        usage_is_incomplete = bool(usage.get("usageIsIncomplete"))
        recorded_at = int(updates_path.stat().st_mtime) if updates_path.exists() else now

        # Relabel Volcengine-style sentinels (`"auto"`) using the freshest
        # non-sentinel `modelId` seen in this session, so `ark-code-latest`
        # spend stops collapsing into a single `auto` row.
        configured = read_configured_model(updates_path)
        if isinstance(model_usage, dict):
            model_usage = relabel_sentinels(model_usage, configured)

        # Mirror `record_session_usage`: prefer per-model breakdown; fall
        # back to a single "unknown" row when only totals are known.
        if not model_usage and int(totals.get("totalTokens", 0)) > 0:
            fallback_key = configured or "unknown"
            model_usage = {fallback_key: totals}

        if not model_usage:
            continue

        for model, m in model_usage.items():
            if not isinstance(m, dict):
                continue
            if dry_run:
                print(
                    f"[dry-run] {session_id} {model} "
                    f"in={m.get('inputTokens', 0)} "
                    f"out={m.get('outputTokens', 0)} "
                    f"calls={m.get('modelCalls', 0)}"
                )
            else:
                upsert(conn, session_id, model, recorded_at, m, usage_is_incomplete)
            inserted_rows += 1
        updated_sessions += 1

    if not dry_run:
        conn.commit()
    conn.close()

    print(
        f"scanned={scanned} sessions_written={updated_sessions} "
        f"rows_upserted={inserted_rows} subagent_skipped={skipped_subagent} "
        f"db={db_path}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
