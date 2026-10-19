//! The settings panel's two claims about the host, checked against the host.
//!
//! The panel shows facts about the process serving it and a list of what Safe
//! Web Mode withholds. Both come over the real socket, so this file asks the
//! socket rather than the code that fills the struct in:
//! - `host_info_reports_the_process_that_answered` proves the facts arrive, that
//!   Safe Web Mode answers rather than refusing them, and that the engine
//!   overwrites a host that reports a store or workspace it was never given;
//! - `the_refusal_list_matches_what_the_socket_actually_refuses` walks every
//!   client message in the engine's protocol inventory and asserts the socket's
//!   verdict matches the list the panel renders, so the panel cannot promise a
//!   refusal that does not happen nor hide one that does.

use axum::serve;
use chaos_engine::{ClientMessage, Engine, HostInfo, ServerMessage, StateBackend};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;
use xai_grok_web::router_with_safe_mode;

/// Every message the engine can be asked flows from a single inventory,
/// [`chaos_engine::client_message_samples`], which is now the only enumeration
/// of the protocol (the TypeScript mirror it used to parse is retired with
/// `apps/chaos-ui`). Field values in the inventory are placeholders: Safe Web
/// Mode decides on the message kind alone, before the engine ever looks at the
/// payload, and `safe_mode_tag` supplies the canonical wire tag.
/// Serves `engine` on a throwaway port and returns the address. The server shuts
/// itself down so a test that returns early cannot leak a listener.
async fn serve_router(
    engine: Engine,
    safe_web_mode: bool,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let _ = serve(listener, router_with_safe_mode(engine, "", safe_web_mode))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(30)).await })
            .await;
    });
    (address, task)
}

/// Opens a socket, sends one message, and returns the first answer.
async fn first_answer(address: &std::net::SocketAddr, message: &ClientMessage) -> ServerMessage {
    ask(address, message)
        .await
        .unwrap_or_else(|reason| panic!("{reason}"))
}

/// The same exchange, but a socket that is not there yet is a value instead of a
/// panic. A server still in the middle of binding answers `ConnectionRefused`
/// right away, so a retry loop cannot be built on `first_answer`.
async fn ask(
    address: &std::net::SocketAddr,
    message: &ClientMessage,
) -> Result<ServerMessage, String> {
    let (mut socket, _) = connect_async(format!("ws://{address}/ws"))
        .await
        .map_err(|error| format!("connect to {address}: {error}"))?;
    let handshake = socket
        .next()
        .await
        .ok_or_else(|| format!("{address}: no handshake"))?
        .map_err(|error| format!("{address}: handshake: {error}"))?;
    let _handshake = handshake;
    socket
        .send(Message::Text(
            serde_json::to_string(message).unwrap().into(),
        ))
        .await
        .map_err(|error| format!("{address}: send: {error}"))?;
    let text = socket
        .next()
        .await
        .ok_or_else(|| format!("{address}: no answer"))?
        .map_err(|error| format!("{address}: receive: {error}"))?
        .into_text()
        .map_err(|error| format!("{address}: text: {error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("{address}: decode: {error}"))
}

fn refused_code(answer: &ServerMessage) -> Option<String> {
    match answer {
        ServerMessage::Error { code, .. } => Some(code.clone()),
        _ => None,
    }
}

#[tokio::test]
async fn host_info_reports_the_process_that_answered() {
    let workspace = tempdir().unwrap();
    let engine = Engine::with_workspace(workspace.path())
        .unwrap()
        .with_host_info(|info: &mut HostInfo| {
            info.bind_addr = "127.0.0.1:9911".into();
            info.safe_web_mode = true;
            info.token_required = true;
            info.public_origin = Some("chaos.example.test".into());
            info.preview_ports = vec![3000, 5173];
            // A host is not allowed to report fewer refusals than the transport
            // enforces, so this is overwritten rather than trusted.
            info.safe_mode_refusals = Vec::new();
            // Both of these are lies too: the engine knows what it was handed,
            // and the panel must not be able to be wrong about persistence or
            // about which workspace it can touch.
            info.state_backend = StateBackend::Sqlite;
            info.workspace_root = Some("/somewhere/else".into());
        });
    let (address, server) = serve_router(engine.clone(), true).await;

    let answer = first_answer(
        &address,
        &ClientMessage::GetHostInfo {
            client_msg_id: Uuid::new_v4().to_string(),
        },
    )
    .await;
    let ServerMessage::HostInfo { info } = answer else {
        panic!("safe mode answered get_host_info with {answer:?}, not host_info");
    };
    server.abort();

    assert_eq!(info.bind_addr, "127.0.0.1:9911");
    assert!(info.safe_web_mode, "the host asked for safe mode");
    assert!(info.token_required, "the host asked for a token");
    assert_eq!(info.public_origin.as_deref(), Some("chaos.example.test"));
    assert_eq!(info.preview_ports, vec![3000, 5173]);
    assert_eq!(info.protocol_version, chaos_engine::PROTOCOL_VERSION);
    // An engine with no store keeps nothing, whatever the host claimed.
    assert_eq!(info.state_backend, StateBackend::Memory);
    assert_eq!(
        info.workspace_root.as_deref(),
        Some(
            dunce::canonicalize(workspace.path())
                .unwrap()
                .display()
                .to_string()
                .as_str(),
        )
    );
    assert_eq!(
        info.safe_mode_refusals.len(),
        chaos_engine::SAFE_MODE_REFUSALS.len(),
        "a host clearing the refusal list must not be able to blank the panel"
    );
}

#[tokio::test]
async fn every_protocol_message_has_a_sample_to_ask_with() {
    let samples = chaos_engine::client_message_samples();
    assert!(
        samples.len() >= 52,
        "the protocol inventory only yielded {} samples",
        samples.len()
    );
    let mut seen = std::collections::HashSet::new();
    for message in &samples {
        let tag = chaos_engine::safe_mode_tag(message);
        assert!(seen.insert(tag), "two samples carry the tag {tag}");
    }
    assert_eq!(seen.len(), samples.len());
}

#[tokio::test]
async fn the_refusal_list_matches_what_the_socket_actually_refuses() {
    let engine = Engine::new();
    let (address, server) = serve_router(engine.clone(), true).await;
    let answer = first_answer(
        &address,
        &ClientMessage::GetHostInfo {
            client_msg_id: Uuid::new_v4().to_string(),
        },
    )
    .await;
    let ServerMessage::HostInfo { info } = answer else {
        panic!("get_host_info was refused: {answer:?}");
    };
    let listed: Vec<&str> = info
        .safe_mode_refusals
        .iter()
        .map(|refusal| refusal.message.as_str())
        .collect();

    let mut disagreement = Vec::new();
    for message in chaos_engine::client_message_samples() {
        let tag = chaos_engine::safe_mode_tag(&message);
        let code = refused_code(&first_answer(&address, &message).await);
        let refused = code.as_deref() == Some("safe_web_mode_blocked");
        if refused && !listed.contains(&tag) {
            disagreement.push(format!("{tag} refused but the panel would not list it"));
        }
        if !refused && listed.contains(&tag) {
            disagreement.push(format!("{tag} listed as refused but the socket allowed it"));
        }
    }
    server.abort();

    // Not asserted per message on purpose: one refusal the panel hides is the
    // interesting case, and the run should name every one of them.
    assert!(
        disagreement.is_empty(),
        "the Safe Web Mode list the panel renders disagrees with the socket: {disagreement:?}"
    );
    // A list that came back empty would satisfy both loops above.
    assert!(
        listed.len() >= 15,
        "expected the refusal list to cover the mutation surface, got {listed:?}"
    )
}

/// Drives the function `chaos-web` actually runs, and checks the answers against
/// the process rather than against a fixture: the address the socket took and the
/// version of the binary that took it. The port is picked by binding and releasing
/// first, because `serve` does not return until the process stops.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_serving_process_reports_its_own_socket_and_version() {
    let probe = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let engine = Engine::new();
    let serving = tokio::spawn(async move {
        xai_grok_web::serve_loopback_with_assets_and_safe_mode(engine, port, false, None).await
    });
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut last_attempt = String::from("never attempted");
    let answer = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let message = ClientMessage::GetHostInfo {
                client_msg_id: Uuid::new_v4().to_string(),
            };
            match tokio::time::timeout(Duration::from_secs(2), ask(&address, &message)).await {
                Ok(Ok(answer)) => break answer,
                Ok(Err(reason)) => last_attempt = reason,
                Err(_) => last_attempt = format!("{address}: silent for 2s"),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "the server never answered get_host_info; last attempt: {last_attempt}; \
             serve task already finished: {}",
            serving.is_finished()
        )
    });
    serving.abort();

    let ServerMessage::HostInfo { info } = answer else {
        panic!("get_host_info was refused: {answer:?}");
    };

    assert_eq!(
        info.bind_addr,
        address.to_string(),
        "the panel must name the socket that answered, not a reconstruction of the request"
    );
    assert_eq!(
        info.host_version,
        xai_grok_web::host_version(),
        "the panel reports the version of the binary serving it"
    );
    assert!(
        !info.safe_web_mode,
        "this server was started without safe mode"
    );
    assert_eq!(info.state_backend, StateBackend::Memory);
    assert_eq!(
        info.workspace_root, None,
        "an engine with no workspace must not name one"
    );
}
