//! Layer-2 stream transform for the CatPaw Remote Agent protocol.
//!
//! Consumes a stream of newline-delimited JSON events (one `Value` per
//! line) and produces [`SamplingEvent`]s. Pure: no I/O, no shell coupling.
//! The accumulator (`xai_catpaw::agent::AgentEventAccumulator`) handles
//! the deduplication of tool traces and the per-message text accumulation.

use std::time::{Duration, Instant};

use futures_util::StreamExt;
use futures_util::stream::{BoxStream, Stream};

use xai_grok_sampling_types::{
    AssistantItem, ConversationItem, ConversationResponse, ResponseModelMetadata, SamplingError,
    StopReason, TokenUsage,
};

use crate::events::{SamplingChannel, SamplingErrorInfo, SamplingEvent};
use crate::metrics::InferenceLatencyStats;
use crate::types::RequestId;

/// Transform a raw CatPaw Remote Agent JSONL event stream into a
/// stream of [`SamplingEvent`]s.
///
/// The output stream emits exactly one terminal event per request:
/// [`SamplingEvent::Completed`] on normal stream end, or
/// [`SamplingEvent::Failed`] on error / idle timeout. Callers must not
/// consume past the terminal event.
pub fn stream_remote_agent<'a>(
    raw_stream: BoxStream<'a, Result<serde_json::Value, SamplingError>>,
    model_metadata: Option<ResponseModelMetadata>,
    request_id: RequestId,
    idle_timeout: Duration,
) -> impl Stream<Item = SamplingEvent> + Send + 'a {
    async_stream::stream! {
        let stream_start = Instant::now();
        let mut chunk_timestamps: Vec<Instant> = Vec::new();

        yield SamplingEvent::StreamStarted {
            request_id: request_id.clone(),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        };

        if let Some(metadata) = model_metadata {
            yield SamplingEvent::ModelMetadata {
                request_id: request_id.clone(),
                metadata,
            };
        }

        let mut accumulator = xai_catpaw::agent::AgentEventAccumulator::new("");
        let mut first_token_emitted = false;
        let mut content_acc = String::new();
        let mut chunk_index = 0u64;
        let mut done = false;
        let mut last_event = Instant::now();
        let mut terminal_status: Option<String> = None;
        let mut terminal_error: Option<String> = None;

        let mut raw_stream = raw_stream;
        while let Some(chunk) = raw_stream.next().await {
            match chunk {
                Ok(value) => {
                    let delta = accumulator.ingest(&value);
                    if !delta.content.is_empty() {
                        if !first_token_emitted {
                            yield SamplingEvent::FirstToken {
                                request_id: request_id.clone(),
                            };
                            first_token_emitted = true;
                        }
                        chunk_index += 1;
                        chunk_timestamps.push(Instant::now());
                        content_acc.push_str(&delta.content);
                        yield SamplingEvent::ChannelToken {
                            request_id: request_id.clone(),
                            channel: SamplingChannel::Text,
                            text: delta.content,
                            chunk_index,
                        };
                        last_event = Instant::now();
                    }
                    if let Some(status) = delta.status {
                        terminal_status = Some(status.clone());
                        if matches!(status.as_str(), "completed" | "canceled") {
                            done = true;
                        }
                        last_event = Instant::now();
                    }
                    if let Some(err) = delta.error {
                        terminal_error = Some(err);
                        last_event = Instant::now();
                    }
                    if delta.done {
                        done = true;
                        break;
                    }
                }
                Err(error) => {
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&error),
                    };
                    return;
                }
            }
            if last_event.elapsed() > idle_timeout {
                yield SamplingEvent::Failed {
                    request_id: request_id.clone(),
                    error: SamplingErrorInfo::from(&SamplingError::IdleTimeout {
                        elapsed_secs: idle_timeout.as_secs(),
                    }),
                };
                return;
            }
        }

        // If the upstream returned a status that indicates an error but
        // did not also surface a hard error, surface it as a Failed
        // event so the session does not silently treat it as success.
        if let Some(err) = terminal_error {
            let synthetic = SamplingError::EventStreamError(format!(
                "CatPaw Remote Agent reported error: {err}"
            ));
            yield SamplingEvent::Failed {
                request_id: request_id.clone(),
                error: SamplingErrorInfo::from(&synthetic),
            };
            return;
        }
        if let Some(status) = &terminal_status
            && !matches!(status.as_str(), "completed" | "canceled" | "")
        {
            let synthetic = SamplingError::EventStreamError(format!(
                "CatPaw Remote Agent ended with non-terminal status: {status}"
            ));
            yield SamplingEvent::Failed {
                request_id: request_id.clone(),
                error: SamplingErrorInfo::from(&synthetic),
            };
            return;
        }

        let stream_end = Instant::now();
        let metrics =
            InferenceLatencyStats::from_timestamps(stream_start, &chunk_timestamps, stream_end);

        let items = vec![ConversationItem::Assistant(AssistantItem {
            content: std::sync::Arc::<str>::from(content_acc),
            tool_calls: Vec::new(),
            model_id: None,
            model_fingerprint: None,
            reasoning_effort: None,
        })];

        let response = ConversationResponse {
            items,
            stop_reason: if done {
                Some(StopReason::Stop)
            } else {
                None
            },
            usage: Some(TokenUsage::default()),
            cost_usd_ticks: None,
            message_chunks_emitted: chunk_index,
            doom_loop_signals: Vec::new(),
            stop_message: None,
        };

        yield SamplingEvent::Completed {
            request_id: request_id.clone(),
            response: Box::new(response),
            metrics,
        };
    }
}
