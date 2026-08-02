//! Layer-2 stream transform for the Chat Completions API.
//!
//! Consumes a raw `ChatCompletionChunk` stream and produces
//! [`SamplingEvent`]s. Pure: no I/O, no shell coupling.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use futures_util::stream::{BoxStream, Stream};

use xai_grok_sampling_types::{
    AssistantItem, ChatCompletionChunk, ConversationItem, ConversationResponse,
    ResponseModelMetadata, SamplingError, StopReason, TokenUsage, ToolCall,
};

use crate::events::{SamplingChannel, SamplingErrorInfo, SamplingEvent};
use crate::metrics::InferenceLatencyStats;
use crate::types::RequestId;

/// Transform a raw Chat Completions chunk stream into a stream of
/// [`SamplingEvent`]s.
///
/// The output stream emits exactly one terminal event per request:
/// [`SamplingEvent::Completed`] on normal stream end, or
/// [`SamplingEvent::Failed`] on error / idle timeout. Callers must not
/// consume past the terminal event (the implementation `return`s after
/// yielding it).
///
/// `idle_timeout` covers two cases:
/// 1. The transport stops yielding chunks at all (`tokio::time::timeout`).
/// 2. The transport keeps yielding empty / keepalive chunks but no
///    meaningful content (separate `last_content_chunk_at` timer).
///
/// Both produce `SamplingEvent::Failed { kind: IdleTimeout }`.
pub fn stream_chat_completions<'a>(
    raw_stream: BoxStream<'a, Result<ChatCompletionChunk, SamplingError>>,
    model_metadata: Option<ResponseModelMetadata>,
    request_id: RequestId,
    idle_timeout: Duration,
    // When true, `delta.content` is scanned for inline
    // `<think>...</think>` pseudo-XML tags and the wrapped text is
    // emitted via `SamplingChannel::Reasoning` instead of `Text`.
    // See [`InlineThinkParser`] for the partial-buffer handling.
    // Default-friendly: `false` makes the parser a no-op (zero
    // overhead, identical to the pre-feature behavior).
    extract_inline_thinking: bool,
) -> impl Stream<Item = SamplingEvent> + Send + 'a {
    async_stream::stream! {
        let stream_start = Instant::now();
        let mut chunk_timestamps: Vec<Instant> = Vec::new();

        // Emit StreamStarted before reading any chunks so subscribers
        // can record TTFB / TTLB baselines.
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

        // Per-response accumulators
        let mut first_chunk_seen = false;
        let mut first_choice_seen = false;
        let mut first_token_emitted = false;
        let mut model: String = String::new();
        let mut model_fingerprint: Option<String> = None;
        let mut usage: Option<TokenUsage> = None;
        let mut cost_usd_ticks: Option<i64> = None;
        let mut finish_reason: Option<StopReason> = None;

        let mut content_acc = String::new();
        let mut reasoning_acc = String::new();
        // Tool call deltas keyed by positional index. Each entry is
        // (id, name, arguments_buffer); the first chunk for an index
        // carries id+name and starts the arguments buffer, subsequent
        // chunks append to arguments only.
        let mut tool_call_acc: BTreeMap<u32, (String, String, String)> = BTreeMap::new();

        // Index counter spanning text + reasoning chunks (matches the
        // shell's chunk_index used for notification correlation).
        let mut chunk_index: u64 = 0;
        // Separate counter for AgentMessageChunk (text-only) emissions;
        // mirrored onto ConversationResponse.message_chunks_emitted so
        // downstream can detect lost-streaming-events scenarios.
        let mut message_chunk_count: u64 = 0;

        // Content-aware idle timer: the outer
        // `tokio::time::timeout(idle_timeout, stream.next())` already
        // catches "transport stops yielding chunks". This second timer
        // catches the more subtle case where the model keeps emitting
        // keepalive / empty-delta SSE events that satisfy the outer
        // timer but make no real progress -- some inference engines
        // do exactly that.
        let mut last_content_chunk_at = Instant::now();

        // State machine for inline `<think>...</think>` extraction,
        // constructed only when the flag is on so the parser never
        // allocates in the default-off path.
        let mut think_parser = extract_inline_thinking.then(InlineThinkParser::new);

        let mut stream = raw_stream;
        loop {
            let next = match tokio::time::timeout(idle_timeout, stream.next()).await {
                Ok(Some(next)) => next,
                Ok(None) => break, // stream ended normally
                Err(_elapsed) => {
                    let err = SamplingError::IdleTimeout {
                        elapsed_secs: idle_timeout.as_secs(),
                    };
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }
            };
            let chunk = match next {
                Ok(chunk) => chunk,
                Err(err) => {
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }
            };

            if !first_chunk_seen {
                model = chunk.model.clone();
                model_fingerprint = chunk
                    .system_fingerprint
                    .clone()
                    .filter(|s| !s.is_empty());
                first_chunk_seen = true;
            }

            if let Some(u) = chunk.usage.clone() {
                // Wire cost is cumulative for the response, so last-write-wins.
                // Never clobber a known cost with missing/unreported.
                let chunk_cost = xai_grok_sampling_types::reported_cost_ticks(u.cost_in_usd_ticks);
                cost_usd_ticks = match (cost_usd_ticks, chunk_cost) {
                    (_, Some(n)) => Some(n),
                    (prev, None) => prev,
                };
                usage = Some(u.into());
            }

            // Track whether this chunk carried meaningful content.
            // Set inside the choices loop and checked at the end.
            let mut chunk_has_content = false;

            for choice in chunk.choices.into_iter() {
                first_choice_seen = true;
                if let Some(fr) = choice.finish_reason {
                    finish_reason = Some(fr.into());
                    chunk_has_content = true;
                }

                let delta = choice.delta;

                if let Some(text) = delta.content
                    && !text.is_empty()
                {
                    if !first_token_emitted {
                        first_token_emitted = true;
                        yield SamplingEvent::FirstToken {
                            request_id: request_id.clone(),
                        };
                    }
                    chunk_has_content = true;
                    chunk_timestamps.push(Instant::now());

                    if let Some(parser) = think_parser.as_mut() {
                        // Split the chunk on inline think tags; each
                        // emitted piece goes to its appropriate channel.
                        // The parser persists across chunks so a tag
                        // straddling two feeds is still recognized.
                        for (channel, piece) in parser.feed(&text) {
                            chunk_index += 1;
                            match channel {
                                SamplingChannel::Text => {
                                    message_chunk_count += 1;
                                    content_acc.push_str(&piece);
                                }
                                SamplingChannel::Reasoning => {
                                    reasoning_acc.push_str(&piece);
                                }
                            }
                            yield SamplingEvent::ChannelToken {
                                request_id: request_id.clone(),
                                channel,
                                text: piece,
                                chunk_index,
                            };
                        }
                    } else {
                        chunk_index += 1;
                        message_chunk_count += 1;
                        content_acc.push_str(&text);
                        yield SamplingEvent::ChannelToken {
                            request_id: request_id.clone(),
                            channel: SamplingChannel::Text,
                            text,
                            chunk_index,
                        };
                    }
                }

                if let Some(thought) = delta.reasoning_content
                    && !thought.is_empty()
                {
                    if !first_token_emitted {
                        first_token_emitted = true;
                        yield SamplingEvent::FirstToken {
                            request_id: request_id.clone(),
                        };
                    }
                    chunk_has_content = true;
                    chunk_index += 1;
                    reasoning_acc.push_str(&thought);
                    yield SamplingEvent::ChannelToken {
                        request_id: request_id.clone(),
                        channel: SamplingChannel::Reasoning,
                        text: thought,
                        chunk_index,
                    };
                }

                for tc_delta in delta.tool_calls.into_iter() {
                    chunk_has_content = true;

                    let entry = tool_call_acc
                        .entry(tc_delta.index)
                        .or_insert_with(|| (String::new(), String::new(), String::new()));

                    let mut id_for_event: Option<String> = None;
                    let mut name_for_event: Option<String> = None;
                    let mut args_for_event: Option<String> = None;

                    if let Some(id) = tc_delta.id {
                        if !id.is_empty() {
                            entry.0 = id.clone();
                        }
                        id_for_event = Some(id);
                    }
                    if let Some(func) = tc_delta.function {
                        // Some OpenAI-compatible providers emit the tool
                        // name only in the first delta and then re-send
                        // `function.name: ""` in every subsequent argument
                        // delta. Blindly overwriting would clobber the real
                        // name with an empty string, producing an
                        // undispatchable `ToolCall { name: "" }`. Only
                        // accept a non-blank name; keep the first one seen.
                        if let Some(name) = func.name.as_deref()
                            && !name.trim().is_empty()
                        {
                            entry.1 = name.to_string();
                        }
                        if let Some(name) = func.name {
                            name_for_event = Some(name);
                        }
                        if let Some(args) = func.arguments {
                            entry.2.push_str(&args);
                            args_for_event = Some(args);
                        }
                    }

                    yield SamplingEvent::ToolCallDelta {
                        request_id: request_id.clone(),
                        tool_index: tc_delta.index,
                        id: id_for_event,
                        name: name_for_event,
                        arguments_delta: args_for_event,
                    };
                }
            }

            if chunk_has_content {
                last_content_chunk_at = Instant::now();
            } else if last_content_chunk_at.elapsed() > idle_timeout {
                let err = SamplingError::IdleTimeout {
                    elapsed_secs: idle_timeout.as_secs(),
                };
                yield SamplingEvent::Failed {
                    request_id: request_id.clone(),
                    error: SamplingErrorInfo::from(&err),
                };
                return;
            }
        }

        // ── Build the final response ─────────────────────────────────
        // Some OpenAI-compatible providers stream a `tool_calls` delta that
        // carries arguments (and sometimes an id) but never a
        // `function.name`. A tool call with a blank name cannot be
        // dispatched, and the malformed output is deterministic on replay,
        // so fail fast with a dedicated non-retryable error instead of
        // emitting a `ToolCall { name: "" }` that later dies with a
        // confusing "Tool not found: " at the dispatch layer.
        for (tc_id, tc_name, _tc_args) in tool_call_acc.values() {
            if tc_name.trim().is_empty() {
                let err = SamplingError::MalformedToolCall {
                    tool_call_id: tc_id.clone(),
                };
                yield SamplingEvent::Failed {
                    request_id: request_id.clone(),
                    error: SamplingErrorInfo::from(&err),
                };
                return;
            }
        }

        let tool_calls: Vec<ToolCall> = tool_call_acc
            .into_values()
            .map(|(id, name, arguments)| ToolCall {
                id: std::sync::Arc::<str>::from(id),
                name,
                arguments: std::sync::Arc::<str>::from(arguments),
            })
            .collect();

        // Honor tool calls by overriding the stop reason if the model
        // forgot to set it (mirrors the shell's behavior).
        if !tool_calls.is_empty() {
            finish_reason = Some(StopReason::ToolCalls);
        }

        // Flush any pending inline-think state before building items.
        // If the model hit `max_tokens` mid-reasoning, the tail
        // (e.g. a half-written `</think` tag and any unwrapped
        // reasoning text) lands in the correct channel instead of
        // being silently dropped from the persisted response.
        if let Some(parser) = think_parser.as_mut()
            && let Some((channel, piece)) = parser.flush()
        {
            chunk_index += 1;
            match channel {
                SamplingChannel::Text => {
                    message_chunk_count += 1;
                    content_acc.push_str(&piece);
                }
                SamplingChannel::Reasoning => {
                    reasoning_acc.push_str(&piece);
                }
            }
            yield SamplingEvent::ChannelToken {
                request_id: request_id.clone(),
                channel,
                text: piece,
                chunk_index,
            };
        }

        // Build the trailing Assistant + any reasoning sibling.
        let mut items: Vec<ConversationItem> = Vec::new();
        if first_choice_seen {
            if !reasoning_acc.is_empty() {
                items.push(ConversationItem::Reasoning(
                    xai_grok_sampling_types::synthesized_reasoning_item(reasoning_acc),
                ));
            }
            items.push(ConversationItem::Assistant(AssistantItem {
                content: std::sync::Arc::<str>::from(content_acc),
                tool_calls,
                model_id: Some(model),
                model_fingerprint,
                // Chat Completions does not echo the applied reasoning effort.
                reasoning_effort: None,
            }));
        } else {
            items.push(ConversationItem::assistant(""));
        }

        let stream_end = Instant::now();
        let metrics =
            InferenceLatencyStats::from_timestamps(stream_start, &chunk_timestamps, stream_end);

        let response = ConversationResponse {
            items,
            stop_reason: finish_reason,
            usage,
            cost_usd_ticks,
            message_chunks_emitted: message_chunk_count,
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

// =============================================================================
// InlineThinkParser
// =============================================================================

/// Splits a stream of text chunks on `<think>...</think>` pseudo-XML
/// tags, routing the wrapped content through
/// [`SamplingChannel::Reasoning`] and the rest through
/// [`SamplingChannel::Text`].
///
/// Chinese reasoning models (DeepSeek-R1, Qwen3-Thinking, GLM-Z1)
/// emit reasoning inline in `content` instead of via a structured
/// `reasoning_content` field; this parser lifts that inline reasoning
/// into the same channel the TUI already knows how to fold.
///
/// # Partial-buffer handling
///
/// `<think>` (7 chars) and `</think>` (8 chars) can be split across
/// SSE chunks. The parser keeps only the longest trailing substring
/// that is also a prefix of the tag currently being sought. The
/// buffer is therefore bounded to at most 7 bytes, while an unrelated
/// `<` is emitted immediately as ordinary content.
///
/// # Unclosed `<think>`
///
/// If `flush` is called while the parser is in the reasoning half
/// (e.g. the model hit `max_tokens` mid-reasoning, or the stream was
/// truncated), the buffered tail is flushed as reasoning. This is
/// the safer default — surfacing partial reasoning beats dropping it
/// silently.
struct InlineThinkParser {
    in_thinking: bool,
    /// Pending bytes at the end of the previous feed that could be
    /// the beginning of the tag currently being sought. This is the
    /// longest suffix that is also a proper prefix of that tag, so it
    /// is bounded to at most `CLOSE_TAG.len() - 1` bytes.
    tail: String,
}

const OPEN_TAG: &str = "<think>";
const CLOSE_TAG: &str = "</think>";

impl InlineThinkParser {
    fn new() -> Self {
        Self {
            in_thinking: false,
            tail: String::new(),
        }
    }

    /// Feed a chunk; return a list of (channel, piece) slices in
    /// arrival order. The tail bytes that didn't yet form a complete
    /// tag are kept internally for the next feed.
    fn feed(&mut self, chunk: &str) -> Vec<(SamplingChannel, String)> {
        // Prepend the tail from the previous feed so a tag split
        // across two chunks is re-scanned from its start.
        let mut buf = std::mem::take(&mut self.tail);
        buf.push_str(chunk);

        let mut out = Vec::new();
        let mut i = 0;
        let bytes = buf.as_str();
        while i < bytes.len() {
            let needle = if self.in_thinking {
                CLOSE_TAG
            } else {
                OPEN_TAG
            };
            if let Some(rel) = bytes[i..].find(needle) {
                let end = i + rel;
                if end > i {
                    out.push((self.current_channel(), bytes[i..end].to_string()));
                }
                i = end + needle.len();
                self.in_thinking = !self.in_thinking;
            } else {
                // No complete delimiter remains. Hold back only the
                // longest suffix that could become the tag currently
                // being sought on the next feed. For example, `<th`
                // is retained while `2 < 3` is emitted immediately.
                let remainder = &bytes[i..];
                let tail_len = longest_tag_prefix_suffix(remainder, needle);
                let emit_end = bytes.len() - tail_len;
                if emit_end > i {
                    out.push((self.current_channel(), bytes[i..emit_end].to_string()));
                }
                self.tail = bytes[emit_end..].to_string();
                i = bytes.len();
            }
        }
        out
    }

    /// Drain any remaining buffered text. Emits as the current channel
    /// (Text if outside `<think>`, Reasoning if inside). Called at
    /// stream end so an unclosed `<think>` is preserved rather than
    /// silently dropped.
    fn flush(&mut self) -> Option<(SamplingChannel, String)> {
        if self.tail.is_empty() {
            None
        } else {
            let channel = self.current_channel();
            let text = std::mem::take(&mut self.tail);
            Some((channel, text))
        }
    }

    fn current_channel(&self) -> SamplingChannel {
        if self.in_thinking {
            SamplingChannel::Reasoning
        } else {
            SamplingChannel::Text
        }
    }
}

/// Length of the longest suffix of `text` that is also a proper
/// prefix of `tag`. A full tag is never returned because callers only
/// invoke this after proving that no complete tag remains.
fn longest_tag_prefix_suffix(text: &str, tag: &str) -> usize {
    let max_len = text.len().min(tag.len().saturating_sub(1));
    (1..=max_len)
        .rev()
        .find(|&len| text.ends_with(&tag[..len]))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;
    use std::pin::pin;
    use xai_grok_sampling_types::{
        ChatChunkChoice, ChatChunkDelta, FinishReason, Role, ToolCallDelta as ChunkToolCallDelta,
        ToolCallFunctionDelta, Usage, rs,
    };

    fn rid() -> RequestId {
        RequestId::from("test-req")
    }

    fn make_chunk(deltas: Vec<ChatChunkDelta>) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: "chunk-1".into(),
            object: "chat.completion.chunk".into(),
            created: 0,
            model: "test-model".into(),
            choices: deltas
                .into_iter()
                .enumerate()
                .map(|(i, delta)| ChatChunkChoice {
                    index: i as u32,
                    delta,
                    finish_reason: None,
                })
                .collect(),
            usage: None,
            system_fingerprint: None,
        }
    }

    fn text_chunk(text: &str) -> ChatCompletionChunk {
        make_chunk(vec![ChatChunkDelta {
            role: Some(Role::Assistant),
            content: Some(text.to_string()),
            reasoning_content: None,
            tool_calls: vec![],
            tool_call_id: None,
        }])
    }

    fn final_chunk(reason: FinishReason) -> ChatCompletionChunk {
        let mut chunk = make_chunk(vec![ChatChunkDelta::default()]);
        chunk.choices[0].finish_reason = Some(reason);
        chunk
    }

    async fn collect(s: impl Stream<Item = SamplingEvent>) -> Vec<SamplingEvent> {
        let mut out = Vec::new();
        let mut s = pin!(s);
        while let Some(ev) = s.next().await {
            out.push(ev);
        }
        out
    }

    #[tokio::test]
    async fn empty_stream_yields_started_then_completed() {
        let raw = stream::iter(Vec::<Result<ChatCompletionChunk, SamplingError>>::new()).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            false,
        ))
        .await;

        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], SamplingEvent::StreamStarted { .. }));
        match &events[1] {
            SamplingEvent::Completed { response, .. } => {
                assert!(response.is_empty());
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn text_only_stream_emits_first_token_then_channel_tokens_then_completed() {
        let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
            Ok(text_chunk("Hello, ")),
            Ok(text_chunk("world!")),
            Ok(final_chunk(FinishReason::Stop)),
        ];
        let raw = stream::iter(chunks).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            false,
        ))
        .await;

        // Expected sequence: StreamStarted, FirstToken, ChannelToken(Text)
        // x 2, Completed.
        assert!(matches!(events[0], SamplingEvent::StreamStarted { .. }));
        assert!(matches!(events[1], SamplingEvent::FirstToken { .. }));

        let text_tokens: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                SamplingEvent::ChannelToken {
                    channel: SamplingChannel::Text,
                    text,
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text_tokens, vec!["Hello, ", "world!"]);

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let a = response.assistant().expect("assistant item present");
                assert_eq!(a.content.as_ref(), "Hello, world!");
                assert_eq!(response.stop_reason, Some(StopReason::Stop));
                assert_eq!(response.message_chunks_emitted, 2);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reasoning_chunk_emits_reasoning_channel_and_first_token_once() {
        let mut reasoning_chunk = make_chunk(vec![ChatChunkDelta {
            role: Some(Role::Assistant),
            content: None,
            reasoning_content: Some("thinking...".into()),
            tool_calls: vec![],
            tool_call_id: None,
        }]);
        reasoning_chunk.choices[0].finish_reason = None;

        let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
            Ok(reasoning_chunk),
            Ok(text_chunk("done")),
            Ok(final_chunk(FinishReason::Stop)),
        ];
        let raw = stream::iter(chunks).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            false,
        ))
        .await;

        // FirstToken should appear exactly once.
        let first_token_count = events
            .iter()
            .filter(|e| matches!(e, SamplingEvent::FirstToken { .. }))
            .count();
        assert_eq!(first_token_count, 1);

        let mut saw_reasoning = false;
        let mut saw_text = false;
        for e in &events {
            if let SamplingEvent::ChannelToken { channel, text, .. } = e {
                match channel {
                    SamplingChannel::Reasoning => {
                        assert_eq!(text, "thinking...");
                        saw_reasoning = true;
                    }
                    SamplingChannel::Text => {
                        assert_eq!(text, "done");
                        saw_text = true;
                    }
                }
            }
        }
        assert!(saw_reasoning && saw_text);

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let r = response
                    .reasoning_items()
                    .next()
                    .expect("reasoning sibling preserved");
                let rs::SummaryPart::SummaryText(t) = &r.summary[0];
                assert_eq!(t.text, "thinking...");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tool_call_stream_emits_deltas_and_assembles_final_call() {
        // First chunk has id + name + part of arguments.
        let chunk1 = make_chunk(vec![ChatChunkDelta {
            role: None,
            content: None,
            reasoning_content: None,
            tool_calls: vec![ChunkToolCallDelta {
                index: 0,
                id: Some("call_abc".into()),
                kind: Some("function".into()),
                function: Some(ToolCallFunctionDelta {
                    name: Some("do_thing".into()),
                    arguments: Some("{\"x\":".into()),
                }),
            }],
            tool_call_id: None,
        }]);
        // Second chunk has only argument fragment.
        let chunk2 = make_chunk(vec![ChatChunkDelta {
            role: None,
            content: None,
            reasoning_content: None,
            tool_calls: vec![ChunkToolCallDelta {
                index: 0,
                id: None,
                kind: None,
                function: Some(ToolCallFunctionDelta {
                    name: None,
                    arguments: Some("1}".into()),
                }),
            }],
            tool_call_id: None,
        }]);

        let raw = stream::iter::<Vec<Result<ChatCompletionChunk, SamplingError>>>(vec![
            Ok(chunk1),
            Ok(chunk2),
        ])
        .boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            false,
        ))
        .await;

        let deltas: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                SamplingEvent::ToolCallDelta {
                    tool_index,
                    id,
                    name,
                    arguments_delta,
                    ..
                } => Some((
                    *tool_index,
                    id.clone(),
                    name.clone(),
                    arguments_delta.clone(),
                )),
                _ => None,
            })
            .collect();

        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0].0, 0);
        assert_eq!(deltas[0].1.as_deref(), Some("call_abc"));
        assert_eq!(deltas[0].2.as_deref(), Some("do_thing"));
        assert_eq!(deltas[0].3.as_deref(), Some("{\"x\":"));
        assert_eq!(deltas[1].1, None);
        assert_eq!(deltas[1].2, None);
        assert_eq!(deltas[1].3.as_deref(), Some("1}"));

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let calls = response.tool_calls();
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id.as_ref(), "call_abc");
                assert_eq!(calls[0].name, "do_thing");
                assert_eq!(calls[0].arguments.as_ref(), "{\"x\":1}");
                // Tool calls force ToolCalls stop reason.
                assert_eq!(response.stop_reason, Some(StopReason::ToolCalls));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tool_call_without_name_yields_malformed_tool_call_failure() {
        // Some providers stream a tool call with an id and arguments but
        // never a `function.name`. Such a call cannot be dispatched, so the
        // stream must fail with a non-retryable MalformedToolCall error and
        // must NOT produce a Completed event.
        let chunk = make_chunk(vec![ChatChunkDelta {
            role: None,
            content: None,
            reasoning_content: None,
            tool_calls: vec![ChunkToolCallDelta {
                index: 0,
                id: Some("call_xyz".into()),
                kind: Some("function".into()),
                function: Some(ToolCallFunctionDelta {
                    name: None,
                    arguments: Some("{\"command\":\"memory_pressure\"}".into()),
                }),
            }],
            tool_call_id: None,
        }]);

        let raw = stream::iter::<Vec<Result<ChatCompletionChunk, SamplingError>>>(vec![Ok(chunk)])
            .boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            false,
        ))
        .await;

        let failed = events
            .iter()
            .find_map(|e| match e {
                SamplingEvent::Failed { error, .. } => Some(error),
                _ => None,
            })
            .expect("expected a Failed event");
        assert_eq!(
            failed.kind,
            crate::events::SamplingErrorKind::MalformedToolCall
        );
        assert!(!failed.is_retryable, "malformed tool call must not retry");
        assert!(failed.message.contains("call_xyz"));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SamplingEvent::Completed { .. })),
            "must not emit Completed for a malformed tool call"
        );
    }

    #[tokio::test]
    async fn tool_call_preserves_name_when_later_deltas_send_blank_name() {
        // Regression: some OpenAI-compatible providers (e.g. z1c) send the
        // tool `name` in the first delta and then re-send
        // `function.name: ""` in every subsequent argument delta. The
        // accumulator must keep the first non-blank name instead of
        // clobbering it with the empty string.
        let chunk_name = make_chunk(vec![ChatChunkDelta {
            role: None,
            content: None,
            reasoning_content: None,
            tool_calls: vec![ChunkToolCallDelta {
                index: 0,
                id: Some("call_abc".into()),
                kind: Some("function".into()),
                function: Some(ToolCallFunctionDelta {
                    name: Some("get_weather".into()),
                    arguments: Some(String::new()),
                }),
            }],
            tool_call_id: None,
        }]);
        let chunk_arg = make_chunk(vec![ChatChunkDelta {
            role: None,
            content: None,
            reasoning_content: None,
            tool_calls: vec![ChunkToolCallDelta {
                index: 0,
                id: None,
                kind: None,
                function: Some(ToolCallFunctionDelta {
                    name: Some(String::new()),
                    arguments: Some("{\"city\":\"Paris\"}".into()),
                }),
            }],
            tool_call_id: None,
        }]);

        let raw = stream::iter::<Vec<Result<ChatCompletionChunk, SamplingError>>>(vec![
            Ok(chunk_name),
            Ok(chunk_arg),
        ])
        .boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            false,
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let calls = response.tool_calls();
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id.as_ref(), "call_abc");
                assert_eq!(calls[0].name, "get_weather");
                assert_eq!(calls[0].arguments.as_ref(), "{\"city\":\"Paris\"}");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mid_stream_error_yields_failed_no_completed() {
        let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
            Ok(text_chunk("hi")),
            Err(SamplingError::EventStreamError("conn reset".into())),
        ];
        let raw = stream::iter(chunks).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            false,
        ))
        .await;

        assert!(
            events
                .iter()
                .any(|e| matches!(e, SamplingEvent::Failed { .. }))
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SamplingEvent::Completed { .. }))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn idle_timeout_when_stream_stalls() {
        // A stream that yields one chunk then hangs forever.
        let raw = stream::iter(vec![Ok(text_chunk("hello"))])
            .chain(stream::pending())
            .boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_millis(100),
            false,
        ))
        .await;

        // Stream should emit StreamStarted, FirstToken, ChannelToken
        // then Failed(IdleTimeout) when the stall hits the deadline.
        match events.last().unwrap() {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(error.kind, crate::events::SamplingErrorKind::IdleTimeout);
            }
            other => panic!("expected Failed(IdleTimeout), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn model_metadata_yielded_after_stream_started() {
        let raw = stream::iter(Vec::<Result<ChatCompletionChunk, SamplingError>>::new()).boxed();
        let metadata = ResponseModelMetadata {
            context_window: Some(8192),
            max_completion_tokens: Some(4096),
            models_etag: None,
        };
        let events = collect(stream_chat_completions(
            raw,
            Some(metadata.clone()),
            rid(),
            Duration::from_secs(60),
            false,
        ))
        .await;

        assert!(matches!(events[0], SamplingEvent::StreamStarted { .. }));
        match &events[1] {
            SamplingEvent::ModelMetadata { metadata: m, .. } => {
                assert_eq!(m.context_window, Some(8192));
                assert_eq!(m.max_completion_tokens, Some(4096));
            }
            other => panic!("expected ModelMetadata second, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn usage_is_extracted_from_chunk() {
        let mut chunk_with_usage = make_chunk(vec![ChatChunkDelta::default()]);
        chunk_with_usage.usage = Some(Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            prompt_tokens_details: None,
            completion_tokens_details: None,
            cost_in_usd_ticks: None,
        });

        let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
            Ok(text_chunk("ok")),
            Ok(chunk_with_usage),
            Ok(final_chunk(FinishReason::Stop)),
        ];
        let raw = stream::iter(chunks).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            false,
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let u = response.usage.as_ref().expect("usage extracted");
                assert_eq!(u.prompt_tokens, 100);
                assert_eq!(u.completion_tokens, 50);
                assert_eq!(u.total_tokens, 150);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    /// Server-reported cost lands on the response; the REST mapper's `0`
    /// backfill means "unreported" and must yield `None`.
    #[tokio::test]
    async fn cost_is_extracted_and_zero_is_unreported() {
        for (wire, expected) in [(Some(78), Some(78)), (Some(0), None), (None, None)] {
            let mut chunk_with_usage = make_chunk(vec![ChatChunkDelta::default()]);
            chunk_with_usage.usage = Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
                prompt_tokens_details: None,
                completion_tokens_details: None,
                cost_in_usd_ticks: wire,
            });
            let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
                Ok(text_chunk("ok")),
                Ok(chunk_with_usage),
                Ok(final_chunk(FinishReason::Stop)),
            ];
            let raw = stream::iter(chunks).boxed();
            let events = collect(stream_chat_completions(
                raw,
                None,
                rid(),
                Duration::from_secs(60),
                false,
            ))
            .await;
            match events.last().unwrap() {
                SamplingEvent::Completed { response, .. } => {
                    assert_eq!(response.cost_usd_ticks, expected, "wire {wire:?}");
                }
                other => panic!("expected Completed, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn later_missing_cost_does_not_clobber_earlier_ticks() {
        let mut first = make_chunk(vec![ChatChunkDelta::default()]);
        first.usage = Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            prompt_tokens_details: None,
            completion_tokens_details: None,
            cost_in_usd_ticks: Some(99),
        });
        let mut second = make_chunk(vec![ChatChunkDelta::default()]);
        second.usage = Some(Usage {
            prompt_tokens: 12,
            completion_tokens: 6,
            total_tokens: 18,
            prompt_tokens_details: None,
            completion_tokens_details: None,
            cost_in_usd_ticks: Some(0),
        });
        let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
            Ok(text_chunk("ok")),
            Ok(first),
            Ok(second),
            Ok(final_chunk(FinishReason::Stop)),
        ];
        let raw = stream::iter(chunks).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            false,
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.cost_usd_ticks, Some(99));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    // ---- InlineThinkParser unit tests ----

    fn collect_chunks(
        p: &mut InlineThinkParser,
        chunks: &[&str],
    ) -> Vec<(SamplingChannel, String)> {
        let mut out = Vec::new();
        for chunk in chunks {
            out.extend(p.feed(chunk));
        }
        // Mirror the stream-level call: end-of-stream flushes the
        // remaining tail so an unclosed partial tag is preserved as
        // the current channel's content.
        out.extend(p.flush());
        out
    }

    #[test]
    fn inline_think_disabled_when_flag_off() {
        // When extract_inline_thinking is false the stream function never
        // constructs a parser; verify the helper struct itself stays
        // trivially correct as a smoke test.
        let mut p = InlineThinkParser::new();
        let out = collect_chunks(&mut p, &["<think>hello</think>world"]);
        assert_eq!(
            out,
            vec![
                (SamplingChannel::Reasoning, "hello".to_string()),
                (SamplingChannel::Text, "world".to_string()),
            ]
        );
    }

    #[test]
    fn inline_think_simple_block_splits() {
        let mut p = InlineThinkParser::new();
        let out = p.feed("<think>foo</think>bar");
        assert_eq!(
            out,
            vec![
                (SamplingChannel::Reasoning, "foo".to_string()),
                (SamplingChannel::Text, "bar".to_string()),
            ]
        );
    }

    #[test]
    fn inline_think_text_around_block() {
        let mut p = InlineThinkParser::new();
        let out = p.feed("before<think>middle</think>after");
        assert_eq!(
            out,
            vec![
                (SamplingChannel::Text, "before".to_string()),
                (SamplingChannel::Reasoning, "middle".to_string()),
                (SamplingChannel::Text, "after".to_string()),
            ]
        );
    }

    #[test]
    fn inline_think_open_tag_split_across_chunks() {
        // The opening tag's last byte lands in the second chunk.
        let mut p = InlineThinkParser::new();
        let out = collect_chunks(&mut p, &["<th", "ink>foo</think>rest"]);
        assert_eq!(
            out,
            vec![
                (SamplingChannel::Reasoning, "foo".to_string()),
                (SamplingChannel::Text, "rest".to_string()),
            ]
        );
    }

    #[test]
    fn inline_think_close_tag_split_across_chunks() {
        // The closing tag's last byte lands in the second chunk.
        let mut p = InlineThinkParser::new();
        let out = collect_chunks(&mut p, &["<think>foo</thin", "k>rest"]);
        assert_eq!(
            out,
            vec![
                (SamplingChannel::Reasoning, "foo".to_string()),
                (SamplingChannel::Text, "rest".to_string()),
            ]
        );
    }

    #[test]
    fn inline_think_unclosed_reasoning_emitted_inline() {
        // The `<think>` tag is recognized at the start of the feed,
        // so the parser emits the reasoning body as it goes. The
        // final tail is empty because nothing before the trailing
        // position is a partial tag.
        let mut p = InlineThinkParser::new();
        let out = p.feed("<think>reasoning without close");
        assert_eq!(
            out,
            vec![(
                SamplingChannel::Reasoning,
                "reasoning without close".to_string()
            )]
        );
        assert!(p.flush().is_none());
    }

    #[test]
    fn inline_think_partial_tag_at_end_flushes_as_reasoning() {
        // Feed ends with a partial close tag (`</thin`) — the bytes
        // before it are emitted, the `<` onwards is held in tail.
        // On flush, that held `<…` becomes Reasoning (we're inside
        // `<think>`) rather than Text, so the user's reasoning text
        // isn't accidentally promoted to a visible message.
        let mut p = InlineThinkParser::new();
        let out = p.feed("<think>start</thin");
        assert_eq!(out, vec![(SamplingChannel::Reasoning, "start".to_string())]);
        let flushed = p.flush().expect("partial tag must flush");
        assert_eq!(flushed.0, SamplingChannel::Reasoning);
        assert_eq!(flushed.1, "</thin");
    }

    #[test]
    fn inline_think_open_tag_buffered_when_chunk_boundary_holds_partial() {
        // The chunk ends with `<th` and the next chunk continues with
        // `ink>rest`; the parser must NOT emit `<th` as Text — it's
        // the start of the open tag. Once the tag is confirmed, the
        // following content (`rest`) lands in Reasoning.
        let mut p = InlineThinkParser::new();
        let out1 = p.feed("foo<th");
        assert_eq!(out1, vec![(SamplingChannel::Text, "foo".to_string())]);
        let out2 = p.feed("ink>rest");
        assert_eq!(out2, vec![(SamplingChannel::Reasoning, "rest".to_string())]);
    }

    #[test]
    fn inline_think_nested_open_inside_thinking_is_treated_as_text() {
        // `<think>a<think>b</think>c`:
        //   outer `<think>` opens reasoning; `a<think>b` is reasoning
        //   text (the inner `<think>` is just literal text, not a
        //   nested open because we're already in reasoning and only
        //   look for `</think>`); `</think>` closes reasoning; `c`
        //   is back in text mode. So the inner `<think>` is correctly
        //   left as reasoning content rather than toggling the mode
        //   a second time.
        let mut p = InlineThinkParser::new();
        let out = p.feed("<think>a<think>b</think>c");
        assert_eq!(
            out,
            vec![
                (SamplingChannel::Reasoning, "a<think>b".to_string()),
                (SamplingChannel::Text, "c".to_string()),
            ]
        );
    }

    #[test]
    fn inline_think_unrelated_less_than_is_emitted_without_buffering() {
        let mut p = InlineThinkParser::new();
        let out = p.feed("2 < 3 and <tag>literal</tag>");
        assert_eq!(
            out,
            vec![(
                SamplingChannel::Text,
                "2 < 3 and <tag>literal</tag>".to_string()
            )]
        );
        assert!(p.flush().is_none());
    }

    #[test]
    fn inline_think_keeps_only_longest_delimiter_prefix() {
        let mut p = InlineThinkParser::new();
        assert_eq!(
            p.feed("visible<<thi"),
            vec![(SamplingChannel::Text, "visible<".to_string())]
        );
        assert_eq!(
            p.feed("nk>reasoning</think>answer"),
            vec![
                (SamplingChannel::Reasoning, "reasoning".to_string()),
                (SamplingChannel::Text, "answer".to_string()),
            ]
        );
        assert!(p.flush().is_none());
    }

    #[test]
    fn inline_think_flush_with_empty_tail_returns_none() {
        let mut p = InlineThinkParser::new();
        // No feed → tail is empty; flush is a no-op.
        assert!(p.flush().is_none());
    }

    #[test]
    fn inline_think_flush_after_complete_block_returns_none() {
        // Tail is empty after a complete tag (no partial bytes left).
        let mut p = InlineThinkParser::new();
        let _ = p.feed("<think>foo</think>");
        assert!(p.flush().is_none());
    }

    // ---- stream_chat_completions integration tests for extract_inline_thinking ----

    /// Helper that builds a stream of text chunks containing the
    /// given payload, split at byte boundaries that exercise partial
    /// tag detection. The default splits at the worst possible point
    /// (mid-tag) — callers can override `chunk_size` to control.
    fn split_text(text: &str) -> Vec<Result<ChatCompletionChunk, SamplingError>> {
        // For tests that don't need a specific split, feed the entire
        // string as one chunk — that's the common case for real models.
        vec![Ok(text_chunk(text)), Ok(final_chunk(FinishReason::Stop))]
    }

    /// Helper for partial-buffer tests: feed the text in two chunks
    /// with the split landing mid-tag.
    fn split_text_at(text: &str, mid: usize) -> Vec<Result<ChatCompletionChunk, SamplingError>> {
        let (a, b) = text.split_at(mid);
        vec![
            Ok(text_chunk(a)),
            Ok(text_chunk(b)),
            Ok(final_chunk(FinishReason::Stop)),
        ]
    }

    fn collect_channels(events: &[SamplingEvent]) -> Vec<(SamplingChannel, String)> {
        events
            .iter()
            .filter_map(|e| match e {
                SamplingEvent::ChannelToken { channel, text, .. } => {
                    Some((channel.clone(), text.clone()))
                }
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn stream_extract_inline_thinking_disabled_passes_through_as_text() {
        // Default-off: even if the response contains tags, they flow
        // as Text — preserves pre-feature behavior exactly.
        let raw = stream::iter(split_text("<think>foo</think>bar")).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            false,
        ))
        .await;
        let channels = collect_channels(&events);
        // Whole text arrives as a single Text token (no splitting).
        assert_eq!(
            channels,
            vec![(SamplingChannel::Text, "<think>foo</think>bar".to_string())]
        );
    }

    #[tokio::test]
    async fn stream_extract_inline_thinking_splits_simple_block() {
        let raw = stream::iter(split_text("<think>foo</think>bar")).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            true,
        ))
        .await;
        let channels = collect_channels(&events);
        assert_eq!(
            channels,
            vec![
                (SamplingChannel::Reasoning, "foo".to_string()),
                (SamplingChannel::Text, "bar".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn stream_extract_inline_thinking_handles_split_open_tag() {
        // Split the chunk so the closing `>` of `<think>` lands in the
        // second chunk — exercises the partial-buffer path. The body
        // text `deep reasoning` lands entirely in the second chunk so
        // it's not fragmented by the boundary (otherwise the parser
        // would have to wait for the next feed to confirm the body's
        // `n` isn't part of a tag — a real chunk boundary inside a
        // body word would be unusual but the parser would still emit
        // it correctly, just in two pieces).
        let payload = "<think>deep reasoning</think>answer";
        // Split after `<th` — first chunk = `<th`, second = `ink>deep
        // reasoning</think>answer`. The body lands in the second
        // chunk as a single Reasoning piece.
        let split_at = 3;
        let raw = stream::iter(split_text_at(payload, split_at)).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            true,
        ))
        .await;
        let channels = collect_channels(&events);
        assert_eq!(
            channels,
            vec![
                (SamplingChannel::Reasoning, "deep reasoning".to_string()),
                (SamplingChannel::Text, "answer".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn stream_extract_inline_thinking_unclosed_reasoning_emitted() {
        // Model hit `max_tokens` mid-reasoning — no closing tag.
        // The opening tag is recognized on the first chunk; the body
        // is emitted inline as Reasoning. No tail remains because
        // the body has no trailing partial tag.
        let raw = stream::iter(split_text("<think>partial reasoning")).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            true,
        ))
        .await;
        let channels = collect_channels(&events);
        assert_eq!(
            channels,
            vec![(SamplingChannel::Reasoning, "partial reasoning".to_string())]
        );
    }

    #[tokio::test]
    async fn stream_extract_inline_thinking_assistant_and_reasoning_items_correct() {
        // Verify the persisted ConversationResponse items reflect the
        // split: a Reasoning sibling + an Assistant with content stripped
        // of the think block.
        let raw = stream::iter(split_text("<think>r</think>visible")).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            true,
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                // Expect: Reasoning("r"), Assistant("visible")
                assert_eq!(response.items.len(), 2);
                match &response.items[0] {
                    xai_grok_sampling_types::ConversationItem::Reasoning(item) => {
                        // The text of a Reasoning item carries the raw
                        // reasoning; verify it equals "r".
                        assert_eq!(
                            xai_grok_sampling_types::reasoning_item_text(item),
                            "r",
                            "first item should be reasoning"
                        );
                    }
                    other => panic!("expected Reasoning, got {other:?}"),
                }
                match &response.items[1] {
                    xai_grok_sampling_types::ConversationItem::Assistant(a) => {
                        assert_eq!(a.content.as_ref(), "visible");
                        assert!(a.tool_calls.is_empty());
                    }
                    other => panic!("expected Assistant, got {other:?}"),
                }
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }
}
