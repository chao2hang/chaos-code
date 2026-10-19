//! 「使用统计」这一路的端到端：真实 WebSocket、官方二进制帧、引擎里真跑的一轮。
//!
//! `chaos_bridge` 里的单元测试只证明帧会转给 worker、worker 会去问引擎；这条把
//! 中间所有东西都换成真的：axum 真的起在一个端口，客户端按官方帧格式发
//! `usage-stats.getAppUsageSnapshot`，引擎里由一个会回报 `usage` 的 adapter 真跑
//! 了一轮。设置页看到的那串数字必须是这一轮报出来的，不是补出来的。

use std::time::Duration;

use chaos_engine::{
    ClientMessage, Engine, PromptAdapter, ProviderUsage, RunOptions, ServerMessage, TurnReport,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;
use zcode_rpc_protocol::{RpcValue, read_message, rpc_type, write_frame, write_message};

/// 只在 `usage` 里报两个数、别的一概不编的 Provider。
struct Reporting;

impl PromptAdapter for Reporting {
    fn run_prompt(&self, _prompt: &str, _options: &RunOptions) -> Result<Vec<String>, String> {
        Ok(vec!["好的".to_string()])
    }

    fn run_prompt_turn(
        &self,
        prompt: &str,
        options: &RunOptions,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<TurnReport, String> {
        for chunk in self.run_prompt(prompt, options)? {
            if !emit(&chunk) {
                break;
            }
        }
        Ok(TurnReport {
            usage: Some(ProviderUsage {
                input_tokens: 120,
                output_tokens: 34,
                ..Default::default()
            }),
        })
    }

    fn default_model(&self) -> Option<&str> {
        Some("fixture-model")
    }
}

/// 官方 Promise(100) 帧：header `[100, id, channel, name]`，参数在 body。
fn call_frame(id: u32, channel: &str, name: &str, arg: Value) -> Vec<u8> {
    write_frame(&write_message(
        &RpcValue::Json(json!([rpc_type::PROMISE, id, channel, name])),
        &RpcValue::Json(arg),
    ))
}

/// 去掉 13 字节帧头，拿到一条 RPC message。
fn message_of(frame: &[u8]) -> Option<(u64, Value)> {
    if frame.len() < 13 {
        return None;
    }
    let len = u32::from_be_bytes(frame[9..13].try_into().ok()?) as usize;
    let (header, body) = read_message(&frame[13..13 + len]).ok()?;
    let RpcValue::Json(Value::Array(items)) = header else {
        return None;
    };
    let kind = items.first()?.as_u64()?;
    let id = items.get(1).and_then(Value::as_u64)?;
    let body = match body {
        RpcValue::Json(value) => value,
        RpcValue::Undefined => Value::Null,
        RpcValue::Binary(bytes, _) => json!({ "binary": bytes }),
    };
    Some((
        id,
        if kind == rpc_type::PROMISE_SUCCESS as u64 {
            body
        } else {
            json!({"refused": body})
        },
    ))
}

#[tokio::test]
async fn usage_stats_snapshot_answers_the_browser_with_real_numbers() {
    let engine = Engine::with_adapter(Reporting);

    // 先来一轮真账：这一轮的 token 数就是稍后快照里该有的数。
    let created = engine.handle(ClientMessage::CreateSession {
        client_msg_id: "usage-e2e-create".into(),
        workspace_id: None,
    });
    let session_id = match &created[0] {
        ServerMessage::SessionCreated { session_id, .. } => *session_id,
        other => panic!("expected SessionCreated, got {other:?}"),
    };
    engine.handle(ClientMessage::Submit {
        client_msg_id: "usage-e2e-submit".into(),
        session_id,
        prompt: "报一下你这轮的用量".into(),
    });

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(30)).await })
            .await;
    });

    let (mut socket, _) = connect_async(format!("ws://{address}/ws/zcode"))
        .await
        .expect("ZCode 端点应当接上");

    // 轮次在引擎自己的工作线程上跑，所以这里按帧重试，不赌它已经落账。
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut id = 1u32;
    let snapshot = loop {
        socket
            .send(Message::Binary(
                call_frame(
                    id,
                    "usage-stats",
                    "getAppUsageSnapshot",
                    json!({ "range": "all", "timeZone": "Asia/Shanghai" }),
                )
                .into(),
            ))
            .await
            .unwrap();
        let answer = loop {
            let frame = match socket.next().await {
                Some(Ok(Message::Binary(bytes))) => bytes,
                Some(Ok(other)) => {
                    panic!("ZCode 端点应当只说二进制帧，收到 {other:?}")
                }
                Some(Err(error)) => panic!("帧读取出错：{error}"),
                None => panic!("连接断了"),
            };
            let Some((answered, body)) = message_of(&frame) else {
                continue; // 服务端的 Initialize 等帧，跟这次调用无关
            };
            if answered != u64::from(id) {
                continue;
            }
            if body["summary"]["totalTokens"].as_u64() == Some(154) {
                break Some(body);
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "快照里始终没出现这一轮的账，最后收到：{body}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
            break None;
        };
        if let Some(snapshot) = answer {
            break snapshot;
        }
        id += 1;
    };

    // 数字来自 Provider 报的那两个值，区间和时区按请求原样回来。
    assert_eq!(snapshot["summary"]["inputTokens"], 120);
    assert_eq!(snapshot["summary"]["outputTokens"], 34);
    assert_eq!(snapshot["summary"]["totalTurns"], 1);
    assert_eq!(snapshot["range"], "all");
    assert_eq!(snapshot["timeZone"], "Asia/Shanghai");
    assert_eq!(snapshot["models"][0]["modelId"], "fixture-model");
}
