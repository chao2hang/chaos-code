//! What a server actually sent and received while it was starting up.
//!
//! A server that fails the handshake leaves async-lsp with one word,
//! `service stopped`, and the client has to supply the rest. The exit status and
//! the stderr tail cover a server that refused to run; they say nothing about a
//! server that ran, answered, and was not understood, which is exactly the
//! failure a text-mode `\n` rewrite produces (`\r\n\r\n` reaching the parser as
//! `\r\r\n\r\r\n`), and nothing about a client that never wrote the request at
//! all. Keeping the first bytes in each direction turns
//! `service stopped; the process exited with code 0` into a named cause.
//!
//! Recording stops as soon as `initialize` is answered. A healthy server streams
//! megabytes of diagnostics through these streams and a startup diagnostic has
//! nothing to learn from any of it, so the steady-state cost is one atomic load
//! per read.

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// How much of the server's answer is kept. Long enough to show the framing of a
/// whole `initialize` response, short enough to belong in one error message.
const MAX_PREVIEW_BYTES: usize = 200;

/// The first bytes in both directions, for one server process.
///
/// Clone it and hand the copies to the stream wrappers; they all report into the
/// same record.
#[derive(Clone)]
pub(crate) struct StartupTrace(Arc<Inner>);

struct Inner {
    /// Bytes the OS accepted for the server's stdin.
    sent: AtomicUsize,
    /// Bytes the server wrote, whether or not they were kept.
    received: AtomicUsize,
    preview: Mutex<Vec<u8>>,
    /// Set once startup is settled, and again when the preview is full.
    quiet: AtomicBool,
}

impl Default for StartupTrace {
    fn default() -> Self {
        Self(Arc::new(Inner {
            sent: AtomicUsize::new(0),
            received: AtomicUsize::new(0),
            preview: Mutex::new(Vec::new()),
            quiet: AtomicBool::new(false),
        }))
    }
}

impl StartupTrace {
    /// Stop keeping the server's output: startup is settled, healthy or not.
    pub(crate) fn disarm(&self) {
        self.0.quiet.store(true, Ordering::Relaxed);
    }

    /// The clause for a startup failure: what went out, and what came back.
    pub(crate) fn summary(&self) -> String {
        let sent = self.0.sent.load(Ordering::Relaxed);
        let received = self.0.received.load(Ordering::Relaxed);
        if received == 0 {
            return format!("; {sent} bytes were sent to it; it wrote nothing on stdout");
        }
        let kept = lock(&self.0.preview);
        format!(
            "; {sent} bytes were sent to it; it wrote {received} bytes on stdout: \"{}\"",
            render(&kept, received > kept.len())
        )
    }

    fn record_read(&self, bytes: &[u8]) {
        self.0.received.fetch_add(bytes.len(), Ordering::Relaxed);
        if bytes.is_empty() || self.0.quiet.load(Ordering::Relaxed) {
            return;
        }
        let mut kept = lock(&self.0.preview);
        let room = MAX_PREVIEW_BYTES.saturating_sub(kept.len());
        let take = room.min(bytes.len());
        kept.extend_from_slice(&bytes[..take]);
        if kept.len() >= MAX_PREVIEW_BYTES {
            self.0.quiet.store(true, Ordering::Relaxed);
        }
    }

    fn record_write(&self, written: usize) {
        self.0.sent.fetch_add(written, Ordering::Relaxed);
    }
}

/// Reads through `inner`, reporting every byte to `trace`.
pub(crate) struct TraceReader<R> {
    inner: R,
    trace: StartupTrace,
}

impl<R> TraceReader<R> {
    pub(crate) fn new(inner: R, trace: StartupTrace) -> Self {
        Self { inner, trace }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for TraceReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let ready = Pin::new(&mut this.inner).poll_read(cx, buf);
        this.trace.record_read(&buf.filled()[before..]);
        ready
    }
}

/// Writes through `inner`, reporting every byte the OS accepts to `trace`.
pub(crate) struct TraceWriter<W> {
    inner: W,
    trace: StartupTrace,
}

impl<W> TraceWriter<W> {
    pub(crate) fn new(inner: W, trace: StartupTrace) -> Self {
        Self { inner, trace }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for TraceWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(written)) => {
                this.trace.record_write(written);
                Poll::Ready(Ok(written))
            }
            other => other,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Mutexes here hold a `Vec` append and a format, so a panic elsewhere while one
/// is held loses a startup diagnostic at worst; recovering the bytes is the right
/// call for a diagnostics path, which must not panic in turn.
fn lock(preview: &Mutex<Vec<u8>>) -> std::sync::MutexGuard<'_, Vec<u8>> {
    preview
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The kept bytes as one line, with the control characters spelled out.
///
/// A raw `\r` in an error message is invisible in a terminal and in a CI log,
/// which is precisely the byte this exists to reveal, so it goes out escaped.
fn render(kept: &[u8], truncated: bool) -> String {
    let mut out = String::with_capacity(kept.len() + 8);
    for ch in String::from_utf8_lossy(kept).chars() {
        if ch == '"' {
            // Escaped by hand: `escape_debug` leaves a double quote alone inside
            // a character escape context, and the preview is wrapped in quotes.
            out.push_str("\\\"");
        } else {
            out.extend(ch.escape_debug());
        }
    }
    if truncated {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// `&[u8]` is an `AsyncRead`, so a reader can be fed a fixed byte string.
    async fn drain<R: AsyncRead + Unpin>(mut reader: R) -> usize {
        let mut sink = Vec::new();
        reader.read_to_end(&mut sink).await.unwrap();
        sink.len()
    }

    #[tokio::test]
    async fn the_first_bytes_are_kept_and_the_rest_is_only_counted() {
        let trace = StartupTrace::default();
        let payload: Vec<u8> = (0..MAX_PREVIEW_BYTES * 3)
            .map(|i| (i % 94 + 33) as u8)
            .collect();
        let read = drain(TraceReader::new(payload.as_slice(), trace.clone())).await;
        assert_eq!(read, payload.len(), "the wrapper moves every byte");
        {
            let kept = lock(&trace.0.preview);
            assert_eq!(
                kept.len(),
                MAX_PREVIEW_BYTES,
                "the preview stays bounded no matter how much the server streams"
            );
            assert_eq!(&kept[..], &payload[..MAX_PREVIEW_BYTES]);
        }
        let summary = trace.summary();
        assert!(
            summary.contains(&format!("it wrote {} bytes on stdout", payload.len())),
            "the count is what the server wrote, not what was kept: {summary}"
        );
        assert!(
            summary.ends_with("…\""),
            "truncation is said, inside the closing quote: {summary}"
        );
    }

    /// The half that keeps a long-lived server from filling a buffer: once
    /// startup is settled, the bytes still flow but none of them are kept.
    #[tokio::test]
    async fn recording_stops_when_startup_is_settled() {
        let trace = StartupTrace::default();
        trace.disarm();
        let read = drain(TraceReader::new(
            b"Content-Length: 2\r\n\r\n{}".as_slice(),
            trace.clone(),
        ))
        .await;
        assert_eq!(read, 23, "the bytes still reach the protocol reader");
        assert!(
            lock(&trace.0.preview).is_empty(),
            "a settled startup has nothing left to diagnose"
        );
        assert!(
            trace
                .summary()
                .contains(&format!("it wrote {read} bytes on stdout")),
            "the count is still honest about traffic: {}",
            trace.summary()
        );
    }

    /// The distinguishing case for the 2026-10-03 Windows failures: a server
    /// that answered in a framing no parser accepted. Without the preview, the
    /// whole report is `service stopped; the process exited with code 0`.
    #[tokio::test]
    async fn the_bytes_that_came_back_survive_into_the_message() {
        let trace = StartupTrace::default();
        drain(TraceReader::new(
            b"Content-Length: 2\r\r\n\r\r\n{}".as_slice(),
            trace.clone(),
        ))
        .await;
        let summary = trace.summary();
        assert!(
            summary.contains("Content-Length: 2\\r\\r\\n"),
            "the doubled carriage return has to be readable in the message: {summary}"
        );
        assert!(
            !summary.contains('\r'),
            "a raw carriage return would be invisible in a log: {summary:?}"
        );
    }

    /// A server that never wrote anything is a different diagnosis from one that
    /// wrote the wrong bytes.
    #[tokio::test]
    async fn a_server_that_stayed_silent_is_said_as_that() {
        let trace = StartupTrace::default();
        assert_eq!(
            trace.summary(),
            "; 0 bytes were sent to it; it wrote nothing on stdout"
        );
    }

    /// What the client sent is the other half of the split: 0 bytes sent means
    /// the request never left, however the server died.
    #[tokio::test]
    async fn what_was_sent_to_the_server_is_counted() {
        let trace = StartupTrace::default();
        let mut writer = TraceWriter::new(tokio::io::sink(), trace.clone());
        writer
            .write_all(b"Content-Length: 2\r\n\r\n{}")
            .await
            .unwrap();
        writer.flush().await.unwrap();
        drop(writer);
        assert!(
            trace
                .summary()
                .contains("23 bytes were sent to it; it wrote nothing on stdout"),
            "{}",
            trace.summary()
        );
    }
}
