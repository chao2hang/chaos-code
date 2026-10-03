use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::{fs, time::Duration};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router_with_safe_mode;

#[tokio::test]
async fn safe_web_mode_blocks_direct_mutation_protocol_calls() {
    let engine = Engine::new();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let served_engine = engine.clone();
    let server = tokio::spawn(async move {
        serve(listener, router_with_safe_mode(served_engine, "", true))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_millis(300)).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let _: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::ProposeGitMutation {
                client_msg_id: "git".into(),
                session_id: uuid::Uuid::new_v4(),
                operation: "discard".into(),
                argument: "note.txt".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let result: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert!(matches!(result, ServerMessage::Error { code, .. } if code == "safe_web_mode_blocked"));
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::ProposeTerminal {
                client_msg_id: "terminal".into(),
                session_id: uuid::Uuid::new_v4(),
                command: "printf blocked".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let result: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert!(matches!(result, ServerMessage::Error { code, .. } if code == "safe_web_mode_blocked"));
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::UpdateSettings {
                client_msg_id: "settings".into(),
                base_url: Some("https://changed.example.test/v1".into()),
                model: Some("changed-model".into()),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let result: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert!(matches!(result, ServerMessage::Error { code, .. } if code == "safe_web_mode_blocked"));
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::GetSettings {
                client_msg_id: "read-settings".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let result: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert!(matches!(
        result,
        ServerMessage::Settings {
            base_url: None,
            model: None,
            ..
        }
    ));
    server.await.unwrap();
}

/// The attachment flow is refused as a unit, from either end.
///
/// `finalize_attachment` used to sit on the Safe Web Mode allowlist while
/// `begin_attachment` did not. That was not a permission but a dead entry: an
/// `upload_id` only ever comes from `begin_attachment`, so the one attachment
/// message that writes into the workspace was reachable in name only, while the
/// three that merely stage bytes were refused. A browser page holding the token
/// could therefore not upload anything -- and if the allowlist had been completed
/// the other way, it could have staged and written whatever the user was then
/// asked to approve, exactly what `propose_file_write` is refused for.
#[tokio::test]
async fn safe_web_mode_refuses_every_step_of_an_attachment_upload() {
    let directory = tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    let engine = Engine::with_workspace(directory.path()).unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router_with_safe_mode(engine, "", true))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(5)).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let _: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();

    // Safe mode still serves what it allows, so a refusal below means "the upload
    // was refused", not "the connection is deaf".
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::CreateSession {
                client_msg_id: "session".into(),
                workspace_id: None,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let session_id =
        match serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
            .unwrap()
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            other => panic!("safe mode refused to create a session: {other:?}"),
        };

    let upload_id = uuid::Uuid::new_v4();
    let steps: [(&str, ClientMessage); 5] = [
        (
            "validate_attachment",
            ClientMessage::ValidateAttachment {
                client_msg_id: "validate".into(),
                filename: "note.txt".into(),
                byte_len: 8,
                content_type: "text/plain".into(),
            },
        ),
        (
            "begin_attachment",
            ClientMessage::BeginAttachment {
                client_msg_id: "begin".into(),
                session_id,
                filename: "note.txt".into(),
                content_type: "text/plain".into(),
                byte_len: 8,
            },
        ),
        (
            "attachment_chunk",
            ClientMessage::AttachmentChunk {
                client_msg_id: "chunk".into(),
                upload_id,
                chunk: "aGVsbG8gd29ybGQ=".into(),
            },
        ),
        (
            "finalize_attachment",
            ClientMessage::FinalizeAttachment {
                client_msg_id: "finalize".into(),
                upload_id,
                relative_path: "note.txt".into(),
            },
        ),
        (
            "cancel_attachment",
            ClientMessage::CancelAttachment {
                client_msg_id: "cancel".into(),
                upload_id,
            },
        ),
    ];
    for (step, message) in steps.iter() {
        socket
            .send(Message::Text(
                serde_json::to_string(message).unwrap().into(),
            ))
            .await
            .unwrap();
        let reply: ServerMessage =
            serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                .unwrap();
        assert!(
            matches!(&reply, ServerMessage::Error { code, .. } if code == "safe_web_mode_blocked"),
            "{step} was not refused: {reply:?}"
        );
    }
    assert!(
        !directory.path().join("note.txt").exists(),
        "a refused upload still wrote into the workspace"
    );
    drop(socket);
    server.await.unwrap();
}
