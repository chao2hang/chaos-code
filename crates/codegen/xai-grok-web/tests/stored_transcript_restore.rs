//! Phase 2.1 的对照验证：真实会话库里的 `updates.jsonl` 必须还原出工具卡、
//! 计划与真实用量——终端跑过工具的会话，到 ZCode 面板上不再是纯文字。
//!
//! 默认不跑（`#[ignore]`）：读的是操作者自己的会话存储；
//! `cargo test -p xai-grok-web --test stored_transcript_restore -- --ignored` 手动跑。

use chaos_engine::Engine;

#[test]
#[ignore = "读真实 ~/.chaos/sessions，需要操作者本机环境"]
fn the_real_store_restores_tool_cards_plan_and_usage() {
    let Some(store) = Engine::chaos_store_dir() else {
        eprintln!("这台机器没有会话存储，跳过");
        return;
    };
    let metas = chaos_engine::chaos_store::scan_store(&store, 50);
    assert!(!metas.is_empty(), "本机应有可扫描会话");

    let mut checked = 0usize;
    let mut with_tools = 0usize;
    let mut with_plan = 0usize;
    let mut with_usage = 0usize;
    for meta in &metas {
        let transcript = chaos_engine::chaos_store::read_updates_transcript(&meta.directory);
        if transcript.rows.is_empty() {
            continue;
        }
        checked += 1;
        let tools = transcript
            .rows
            .iter()
            .filter(|row| matches!(row, chaos_engine::chaos_store::StoredRow::ToolCall { .. }))
            .count();
        if tools > 0 {
            with_tools += 1;
        }
        if transcript.plan.is_some() {
            with_plan += 1;
        }
        if transcript.usage.is_some() {
            with_usage += 1;
        }
        eprintln!(
            "  {:?} rows={} tools={} plan={} usage={:?}",
            meta.directory,
            transcript.rows.len(),
            tools,
            transcript.plan.is_some(),
            transcript
                .usage
                .map(|usage| (usage.input_tokens, usage.output_tokens)),
        );
    }
    eprintln!("对照 {checked} 段：带工具卡 {with_tools}，带计划 {with_plan}，带用量 {with_usage}",);
    assert!(checked > 0, "真实库里应有可还原的会话");
    // 终端跑过工具是常态：至少一段会话还原出工具卡。
    assert!(with_tools > 0, "至少一段会话应还原出工具卡");
    // 完成过的轮次带真实账目：至少一段有用量。
    assert!(with_usage > 0, "至少一段会话应带真实用量");
}
