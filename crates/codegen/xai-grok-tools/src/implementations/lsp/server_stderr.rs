//! The tail of an LSP server's stderr, kept for the moment it fails to start.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

/// How many lines a dead server gets to explain itself with.
const MAX_LINES: usize = 12;

/// Longer lines are cut at this many characters. A crash dump line says nothing
/// useful past this point and would otherwise dominate the error message.
const MAX_LINE_CHARS: usize = 300;

/// The last lines a server wrote to stderr, oldest first.
///
/// A server that dies during `initialize` leaves the caller with async-lsp's
/// `service stopped` and nothing else. The reason is nearly always on stderr,
/// which would otherwise only reach `tracing::debug!` and so be invisible in a
/// default log configuration. Keeping the tail lets the startup failure quote
/// it: the difference between "LSP initialization failed: service stopped" and
/// "rust-analyzer: can't find `Cargo.toml`".
///
/// The socket transport has no stderr; it marks the stream drained up front so
/// nobody waits on a pipe that will never produce one.
#[derive(Clone)]
pub struct ServerStderr {
    lines: Arc<Mutex<Vec<String>>>,
    drained: Arc<tokio::sync::watch::Sender<bool>>,
}

impl Default for ServerStderr {
    fn default() -> Self {
        let (drained, _) = tokio::sync::watch::channel(false);
        Self {
            lines: Arc::new(Mutex::new(Vec::new())),
            drained: Arc::new(drained),
        }
    }
}

impl ServerStderr {
    /// Record one line, dropping the oldest once [`MAX_LINES`] are held.
    pub fn record(&self, line: &str) {
        let line = truncate_chars(line, MAX_LINE_CHARS);
        let mut held = lock(&self.lines);
        held.push(line);
        if held.len() > MAX_LINES {
            let excess = held.len() - MAX_LINES;
            held.drain(..excess);
        }
    }

    /// Say that the stream reached EOF, which releases [`Self::summary`].
    pub fn mark_drained(&self) {
        // Only fails if the receiver half has been dropped, which means nobody
        // is waiting to read the tail anyway.
        let _ = self.drained.send(true);
    }

    /// Render the tail as a clause to append to an error message: empty when the
    /// server said nothing, `; last stderr: <lines>` when it said something.
    ///
    /// A process can have exited while its last lines are still in the pipe, so
    /// this waits for EOF up to `grace` before giving up on the tail.
    pub async fn summary(&self, grace: Duration) -> String {
        let mut drained = self.drained.subscribe();
        let _ = tokio::time::timeout(grace, drained.wait_for(|done| *done)).await;
        let held = lock(&self.lines);
        if held.is_empty() {
            return String::new();
        }
        format!("; last stderr: {}", held.join(" | "))
    }
}

/// Mutexes here are held for a `Vec` push and a `join`, so a panic elsewhere
/// while one is held loses at most a log line; recovering the data is the right
/// call for a diagnostics path, which must not panic in turn.
fn lock(lines: &Arc<Mutex<Vec<String>>>) -> std::sync::MutexGuard<'_, Vec<String>> {
    lines
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Cut to `max` characters without splitting one, on char boundaries.
fn truncate_chars(line: &str, max: usize) -> String {
    match line.char_indices().nth(max) {
        Some((offset, _)) => format!("{}…", &line[..offset]),
        None => line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_keeps_the_last_lines_and_drops_the_oldest() {
        let tail = ServerStderr::default();
        for i in 0..(MAX_LINES + 5) {
            tail.record(&format!("line {i}"));
        }
        let held = lock(&tail.lines);
        assert_eq!(held.len(), MAX_LINES, "the tail must stay bounded");
        assert_eq!(held.first().unwrap().as_str(), "line 5");
        assert_eq!(
            held.last().unwrap().as_str(),
            format!("line {}", MAX_LINES + 4)
        );
    }

    #[test]
    fn an_overlong_line_is_cut_on_a_char_boundary() {
        let tail = ServerStderr::default();
        // Multibyte on purpose: a byte-boundary cut would panic here.
        tail.record(&"é".repeat(MAX_LINE_CHARS + 50));
        let held = lock(&tail.lines);
        let kept = held.first().unwrap();
        assert!(kept.ends_with('…'), "{kept}");
        assert_eq!(
            kept.chars().count(),
            MAX_LINE_CHARS + 1,
            "cut plus ellipsis"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_server_that_said_nothing_contributes_nothing() {
        let tail = ServerStderr::default();
        tail.mark_drained();
        assert_eq!(tail.summary(Duration::from_millis(50)).await, "");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_lines_are_rendered_oldest_first() {
        let tail = ServerStderr::default();
        tail.record("first");
        tail.record("then it died");
        tail.mark_drained();
        assert_eq!(
            tail.summary(Duration::from_millis(50)).await,
            "; last stderr: first | then it died"
        );
    }

    /// The point of the grace period: the process is already gone, the reader
    /// task has not caught up yet. Without waiting, this returns empty.
    #[tokio::test(flavor = "current_thread")]
    async fn the_summary_waits_a_bounded_grace_for_the_last_line() {
        let tail = ServerStderr::default();
        let late = tail.clone();
        let writer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            late.record("the real reason");
            late.mark_drained();
        });
        assert_eq!(
            tail.summary(Duration::from_millis(500)).await,
            "; last stderr: the real reason"
        );
        writer.await.unwrap();
    }

    /// The other half of the grace period: nothing ever arrives, and the caller
    /// must not hang.
    #[tokio::test(flavor = "current_thread")]
    async fn the_summary_gives_up_when_the_stream_never_ends() {
        let tail = ServerStderr::default();
        tail.record("half a story");
        let started = std::time::Instant::now();
        let summary = tail.summary(Duration::from_millis(30)).await;
        assert!(
            started.elapsed() < Duration::from_millis(1_000),
            "the grace period has to be a bound, not a suggestion: {:?}",
            started.elapsed()
        );
        assert_eq!(summary, "; last stderr: half a story");
    }
}
