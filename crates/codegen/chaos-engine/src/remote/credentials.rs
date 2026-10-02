//! One-time session credentials.
//!
//! The transport is a socket, so *reaching* it is not authorisation: whoever
//! can open the file can talk to the server. The token is what turns "reached
//! the socket" into "may use this workspace", and it is deliberately spent on
//! use rather than being a standing password.
//!
//! That choice buys two properties worth having and worth testing. A captured
//! token cannot start a second session, because the first one consumed it. And a
//! token that was never used dies on its own, so a hand-off that went nowhere
//! does not stay valid until somebody notices.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How many spent tokens are remembered.
///
/// A session redeems one token, so this is how far back replay detection
/// reaches. Bounded on purpose: the alternative is memory that grows with every
/// session a long-lived server ever serves.
const SPENT_MEMORY: usize = 4096;

/// A session credential.
///
/// Deliberately without `Display`, and with a redacting `Debug`: this value gets
/// passed through handshake code and error paths, and a token that prints itself
/// is a token that ends up in a log.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl SessionToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Read one from a token file.
    pub fn from_text(text: &str) -> Self {
        Self(text.trim().to_string())
    }
}

impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Enough to tell two tokens apart in a log without being enough to use
        // either.
        write!(
            f,
            "SessionToken({}…)",
            self.0.chars().take(4).collect::<String>()
        )
    }
}

/// Why a credential was not accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RedeemError {
    /// Never issued, or from a different server.
    Unknown,
    /// Issued, and already used for a session.
    Reused,
    /// Issued, but its lifetime has passed.
    Expired,
}

impl std::fmt::Display for RedeemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "the credential was not issued by this server"),
            Self::Reused => write!(
                f,
                "the credential has already opened a session; it is one-time by design"
            ),
            Self::Expired => write!(f, "the credential has expired"),
        }
    }
}

impl std::error::Error for RedeemError {}

/// How a credential left circulation.
///
/// Worth remembering, because the server sweeps stale credentials before it
/// redeems anything: if a swept credential were indistinguishable from a spent
/// one, someone who took too long to connect would be told they had already
/// connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fate {
    /// It opened a session. Presenting it again is a replay.
    Redeemed,
    /// Its lifetime passed without opening one.
    Stale,
}

/// Issues credentials and takes them back out of circulation.
pub struct TokenVault {
    ttl: Duration,
    /// Live tokens, with the instant each was issued.
    live: HashMap<String, Instant>,
    /// Spent or expired tokens, kept longest-first so the oldest can leave.
    retired: VecDeque<String>,
    retired_set: HashMap<String, Fate>,
    now: Box<dyn Fn() -> Instant + Send + Sync>,
}

impl TokenVault {
    /// A vault whose tokens live for `ttl`.
    ///
    /// The clock is injectable so expiry can be tested at millisecond cost
    /// rather than by sleeping for a policy interval that exists for production.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            live: HashMap::new(),
            retired: VecDeque::new(),
            retired_set: HashMap::new(),
            now: Box::new(Instant::now),
        }
    }

    /// Use a caller-supplied clock. For tests and for anything that wants a
    /// virtual clock in the future.
    pub fn with_clock(mut self, now: impl Fn() -> Instant + Send + Sync + 'static) -> Self {
        self.now = Box::new(now);
        self
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Mint a credential.
    pub fn issue(&mut self) -> SessionToken {
        // Two v4 UUIDs: 122 random bits each, which is more than enough that
        // guessing one is not a plan, and neither needs another dependency.
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let issued_at = (self.now)();
        self.live.insert(token.clone(), issued_at);
        SessionToken(token)
    }

    /// Spend a credential on a session. Each token opens exactly one.
    pub fn redeem(&mut self, token: &str) -> Result<(), RedeemError> {
        let now = (self.now)();
        if let Some(issued_at) = self.live.get(token).copied() {
            if now.duration_since(issued_at) >= self.ttl {
                // Retired as stale rather than deleted: it must not come back to
                // life, and a later attempt should still be told "expired"
                // instead of "never issued".
                self.live.remove(token);
                self.retire(token, Fate::Stale);
                return Err(RedeemError::Expired);
            }
            self.live.remove(token);
            self.retire(token, Fate::Redeemed);
            return Ok(());
        }
        // Distinguishing reuse from never-issued is the whole point: one is an
        // attack or a bug, the other is a typo. Stale keeps saying expired,
        // because that is what actually happened to the person who connects
        // late — the server has usually swept the token before they arrive.
        match self.retired_set.get(token) {
            Some(Fate::Redeemed) => Err(RedeemError::Reused),
            Some(Fate::Stale) => Err(RedeemError::Expired),
            None => Err(RedeemError::Unknown),
        }
    }

    /// Credentials still waiting to be used.
    pub fn live_count(&self) -> usize {
        self.live.len()
    }

    /// Drop tokens older than the TTL and say how many went.
    ///
    /// A hand-off that was never redeemed leaves a live credential behind. This
    /// is what makes "it expires" true even for a token nobody presented.
    pub fn sweep_expired(&mut self) -> usize {
        let now = (self.now)();
        let stale: Vec<String> = self
            .live
            .iter()
            .filter(|(_, issued_at)| now.duration_since(**issued_at) >= self.ttl)
            .map(|(token, _)| token.clone())
            .collect();
        let count = stale.len();
        for token in stale {
            self.live.remove(&token);
            self.retire(&token, Fate::Stale);
        }
        count
    }

    fn retire(&mut self, token: &str, fate: Fate) {
        if self.retired_set.len() >= SPENT_MEMORY
            && let Some(oldest) = self.retired.pop_front()
        {
            self.retired_set.remove(&oldest);
        }
        if self.retired_set.insert(token.to_string(), fate).is_none() {
            self.retired.push_back(token.to_string());
        }
    }
}

/// Where the tokens live so a client on the same host can pick one up.
///
/// The file is the hand-off, not the trust boundary: it is created `0600`, so
/// reaching it means already being this user, which is the same thing reaching
/// the socket means. Its contents are the issued tokens, one per line.
///
/// The server writes it once, at startup; it does not rewrite it as tokens are
/// spent, because a partial rewrite would race with a client that is reading it
/// right now. Removing the credential that was just spent is the client's job
/// (see `bin::chaos-remote`), which is the one process that knows which line it
/// actually used. Anyone else picking up a spent line gets a clear refusal, not
/// a session.
pub struct TokenFile {
    path: PathBuf,
}

impl TokenFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write `tokens`, replacing whatever was there.
    pub fn write(&self, tokens: &[SessionToken]) -> Result<(), String> {
        let body: String = tokens
            .iter()
            .map(|token| format!("{}\n", token.as_str()))
            .collect();
        write_private(&self.path, body.as_bytes())
            .map_err(|e| format!("write token file {}: {e}", self.path.display()))?;
        Ok(())
    }

    /// Read every token in the file.
    pub fn read(&self) -> Result<Vec<SessionToken>, String> {
        let text = std::fs::read_to_string(&self.path)
            .map_err(|e| format!("read token file {}: {e}", self.path.display()))?;
        Ok(text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(SessionToken::from_text)
            .collect())
    }
}

/// Create a file with owner-only permission, from the open rather than after
/// the fact: a mode widened after the first write is a mode that was readable
/// while it had contents.
///
/// Written beside the target and renamed onto it. Writing in place would follow
/// a symlink left at that path — this file lives next to a socket, in a directory
/// an operator may have pointed somewhere shared — and `rename` replaces the
/// link itself instead of what it points at. The rename is also why a reader
/// never sees a half-written token list.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "private".to_string()),
        uuid::Uuid::new_v4().simple()
    ));
    let result = write_private_tmp(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn write_private_tmp(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    // create_new, not create: the name carries a UUID, so an existing file at it
    // means something is already racing us for the same directory.
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    let written = std::io::Write::write_all(&mut file, bytes);
    // Windows has no POSIX mode; the owner-only guarantee there comes from the
    // directory the operator picked, and is documented as such rather than
    // pretended to by an attribute that would surprise the next reader.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    written?;
    std::io::Write::flush(&mut file)?;
    drop(file);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    /// A clock the test advances by hand.
    #[derive(Clone, Default)]
    struct FakeClock(Arc<AtomicU64>);

    impl FakeClock {
        fn advance(&self, millis: u64) {
            self.0.fetch_add(millis, Ordering::SeqCst);
        }

        /// `Instant` has no constructor, so every reading is an offset from one
        /// fixed point. Only differences matter to the vault.
        fn instant(&self) -> Instant {
            static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
            let base = *BASE.get_or_init(Instant::now);
            base + Duration::from_millis(self.0.load(Ordering::SeqCst))
        }
    }

    fn vault(ttl: Duration) -> (TokenVault, FakeClock) {
        let clock = FakeClock::default();
        let probe = clock.clone();
        let vault = TokenVault::new(ttl).with_clock(move || probe.instant());
        (vault, clock)
    }

    #[test]
    fn a_token_opens_exactly_one_session() {
        let (mut vault, _clock) = vault(Duration::from_secs(60));
        let token = vault.issue();
        assert!(vault.redeem(token.as_str()).is_ok());
        assert_eq!(
            vault.redeem(token.as_str()),
            Err(RedeemError::Reused),
            "the second use of a spent credential is the replay this design exists to catch"
        );
    }

    #[test]
    fn a_forged_credential_is_not_a_replay() {
        let (mut vault, _clock) = vault(Duration::from_secs(60));
        assert_eq!(vault.redeem("deadbeef"), Err(RedeemError::Unknown));
    }

    #[test]
    fn a_token_dies_when_its_lifetime_passes() {
        let (mut vault, clock) = vault(Duration::from_millis(100));
        let token = vault.issue();
        clock.advance(99);
        assert!(vault.redeem(token.as_str()).is_ok());

        let second = vault.issue();
        clock.advance(101);
        assert_eq!(vault.redeem(second.as_str()), Err(RedeemError::Expired));
        assert_eq!(
            vault.redeem(second.as_str()),
            Err(RedeemError::Expired),
            "a stale credential stays stale; it is not evidence anyone used it"
        );
    }

    /// The server sweeps before it redeems, so in production the expired path is
    /// reached through the retired set rather than the live one. If the sweep
    /// remembered stale credentials the same way it remembers spent ones, anyone
    /// who simply connected late would be told they had already connected.
    #[test]
    fn a_swept_token_is_still_reported_as_expired() {
        let (mut vault, clock) = vault(Duration::from_millis(50));
        let token = vault.issue();
        clock.advance(51);
        assert_eq!(vault.sweep_expired(), 1);
        assert_eq!(
            vault.redeem(token.as_str()),
            Err(RedeemError::Expired),
            "the sweep happens first, so this is the answer a late client actually gets"
        );
    }

    /// A credential handed out and never used is still a live credential. The
    /// expiry has to be real for those too, which means something has to notice.
    #[test]
    fn an_unused_token_is_swept_once_it_is_stale() {
        let (mut vault, clock) = vault(Duration::from_millis(50));
        let _never_used = vault.issue();
        let _also_unused = vault.issue();
        assert_eq!(vault.live_count(), 2);
        clock.advance(51);
        assert_eq!(vault.sweep_expired(), 2);
        assert_eq!(vault.live_count(), 0);
    }

    #[test]
    fn the_spent_memory_is_bounded() {
        let (mut vault, _clock) = vault(Duration::from_secs(60));
        let mut last = None;
        for _ in 0..(SPENT_MEMORY + 10) {
            let token = vault.issue();
            vault.redeem(token.as_str()).unwrap();
            last = Some(token);
        }
        assert!(
            vault.retired.len() <= SPENT_MEMORY,
            "spent set grew without bound: {}",
            vault.retired.len()
        );
        // The most recent spend is still remembered.
        assert_eq!(
            vault.redeem(last.unwrap().as_str()),
            Err(RedeemError::Reused)
        );
    }

    #[test]
    fn tokens_are_not_guessable_and_not_printed() {
        let (mut vault, _clock) = vault(Duration::from_secs(60));
        let a = vault.issue();
        let b = vault.issue();
        assert_ne!(a, b);
        assert!(a.as_str().len() >= 32, "token is suspiciously short");
        let debug = format!("{a:?}");
        assert!(
            !debug.contains(a.as_str()),
            "the debug form must not carry the secret: {debug}"
        );
    }

    #[test]
    fn a_token_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let file = TokenFile::new(dir.path().join("tokens"));
        let (mut vault, _clock) = vault(Duration::from_secs(60));
        let tokens = vec![vault.issue(), vault.issue()];
        file.write(&tokens).unwrap();
        let read = file.read().unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0], tokens[0]);
    }

    /// The file holds a credential, so its permission is part of the design.
    #[test]
    #[cfg(unix)]
    fn a_token_file_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens");
        TokenFile::new(&path)
            .write(&[SessionToken::from_text("abc")])
            .unwrap();
        assert_eq!(file_mode(&path), 0o600);
    }

    #[cfg(unix)]
    fn file_mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// A symlink left at the token path must not be written *through*. Replacing
    /// the link is fine and is what happens; following it is not.
    #[test]
    #[cfg(unix)]
    fn a_token_file_replaces_a_symlink_instead_of_writing_through_it() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "keep").unwrap();
        let link = dir.path().join("tokens");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        TokenFile::new(&link)
            .write(&[SessionToken::from_text("abc")])
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "keep",
            "the link target must be untouched"
        );
        assert!(
            !std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the path is a real file now, not a redirect to somewhere else"
        );
        assert_eq!(file_mode(&link), 0o600);
    }
}
