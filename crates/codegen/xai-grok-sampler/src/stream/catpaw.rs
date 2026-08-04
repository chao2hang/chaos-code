//! Layer-2 stream transform for the CatPaw Chat protocol.
//!
//! Consumes a stream of parsed CatPaw cumulative-SSE JSON events and
//! produces [`SamplingEvent`]s. Pure: no I/O, no shell coupling. The
//! cumulative `content` snapshots are converted to incremental deltas by
//! `xai_catpaw::chat::ChatAccumulator`.

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

/// Transform a raw CatPaw cumulative-SSE event stream into a stream of
/// [`SamplingEvent`]s.
///
/// The output stream emits exactly one terminal event per request:
/// [`SamplingEvent::Completed`] on normal stream end, or
/// [`SamplingEvent::Failed`] on error / idle timeout. Callers must not
/// consume past the terminal event.
pub fn stream_catpaw<'a>(
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

        let mut accumulator = xai_catpaw::chat::ChatAccumulator::new();
        let mut first_token_emitted = false;
        let mut content_acc = String::new();
        let mut usage = TokenUsage::default();
        let mut chunk_index = 0u64;
        let mut done = false;
        let mut last_event = Instant::now();

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
                    if delta.usage.prompt_tokens > 0 || delta.usage.completion_tokens > 0 {
                        usage = TokenUsage {
                            prompt_tokens: delta.usage.prompt_tokens as u32,
                            completion_tokens: delta.usage.completion_tokens as u32,
                            total_tokens: delta.usage.total_tokens as u32,
                            reasoning_tokens: 0,
                            cached_prompt_tokens: 0,
                        };
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
            usage: Some(usage),
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
