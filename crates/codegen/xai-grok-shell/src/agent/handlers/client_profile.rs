//! `x.ai/session/set_client_profile` extension handler.
//!
//! Applies a request-client profile to a live session by sending a
//! `SetClientProfile` command to the session actor. The actor stores the
//! profile's identity fields (client_identifier, origin_client, user_agent)
//! and header maps so they are picked up by `reconstruct_full_config` on
//! the next sampler turn.

use agent_client_protocol as acp;
use tokio::sync::oneshot;

use crate::agent::client_profiles::ClientProfile;
use crate::agent::MvpAgent;
use crate::extensions::{parse_params, to_ext_response, ExtResult};
use crate::session::SessionCommand;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetClientProfileRequest {
    session_id: String,
    profile: ClientProfile,
}

/// Handle `x.ai/session/set_client_profile` — apply a client profile to a
/// live session so subsequent requests carry the profile's identity headers,
/// User-Agent, and client identifier.
pub(crate) async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let req: SetClientProfileRequest = parse_params(args)?;
    let sid: acp::SessionId = req.session_id.clone().into();

    let session_handle = agent.session_handle_waiting_for_load(&sid).await;
    let Some(session) = session_handle else {
        return Err(
            acp::Error::invalid_params()
                .data(format!("session not found: {}", req.session_id)),
        );
    };

    let profile = req.profile;
    let (tx, rx) = oneshot::channel();
    let _ = session.cmd_tx.send(SessionCommand::SetClientProfile {
        client_identifier: profile.client_identifier.clone(),
        origin_client: profile.origin_client(),
        user_agent: profile.user_agent.clone(),
        extra_headers: profile.extra_headers.clone(),
        env_http_headers: profile.env_http_headers.clone(),
        responds_to: tx,
    });

    rx.await
        .map_err(|_| {
            acp::Error::internal_error()
                .data("session actor closed before responding to SetClientProfile")
        })?
        .map_err(|e| acp::Error::internal_error().data(e))?;

    to_ext_response(Ok(serde_json::json!({
        "ok": true,
        "profileId": profile.id,
    })))
}
