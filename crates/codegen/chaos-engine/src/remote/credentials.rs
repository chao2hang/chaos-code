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

    /// A credential that came back from a forward grant rather than from the
    /// server's token file.
    ///
    /// It goes into the same field of the hello, because it is the same *kind* of
    /// thing on the wire — a bearer string the server decides the meaning of. It
    /// cannot open a workspace session: the session vault has never heard of it,
    /// and that is a property of the two stores, not of this conversion.
    pub fn from_forward_ticket(ticket: &ForwardTicket) -> Self {
        Self(ticket.as_str().to_string())
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

/// Where a forwarded connection is allowed to go, spelled the way the server's
/// allowlist spells it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ForwardTarget {
    pub host: String,
    pub port: u16,
}

impl ForwardTarget {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    /// The form worth printing: bracketed for IPv6, so `::1:80` does not read as
    /// two addresses.
    pub fn to_text(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

impl std::fmt::Display for ForwardTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_text())
    }
}

/// A credential for one forwarded connection.
///
/// Same rules as a session credential about not printing itself, for the same
/// reason: it is presented over a socket and passed through handshake code.
#[derive(Clone, PartialEq, Eq)]
pub struct ForwardTicket(String);

impl ForwardTicket {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Read one that arrived over the wire.
    pub fn from_text(text: &str) -> Self {
        Self(text.trim().to_string())
    }
}

impl std::fmt::Debug for ForwardTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ForwardTicket({}…)",
            self.0.chars().take(4).collect::<String>()
        )
    }
}

/// The ceilings a forward grant is clamped to, whatever the caller asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForwardLimits {
    pub max_ttl: Duration,
    pub max_uses: usize,
}

impl Default for ForwardLimits {
    fn default() -> Self {
        Self {
            // Long enough to survive a build or a debugging session, short enough
            // that a forgotten tunnel stops working by itself.
            max_ttl: Duration::from_secs(30 * 60),
            // A browser opening a page can legitimately produce a dozen
            // connections; a port scanner produces thousands.
            max_uses: 256,
        }
    }
}

/// Why a forward ticket did not buy a connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForwardTicketError {
    /// Never issued, or issued by a different server.
    Unknown,
    /// Issued, but every use had been spent.
    Exhausted,
    /// Issued, but its lifetime had passed.
    Expired,
    /// Issued by a session that has since closed.
    Revoked,
}

impl std::fmt::Display for ForwardTicketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "the forward ticket was not issued by this server"),
            Self::Exhausted => write!(
                f,
                "the forward ticket has no connections left to open; ask for another grant"
            ),
            Self::Expired => write!(f, "the forward ticket has expired"),
            Self::Revoked => write!(
                f,
                "the session that authorised this forward has closed, so the forward \
                 closed with it"
            ),
        }
    }
}

impl std::error::Error for ForwardTicketError {}

/// How a forward ticket left circulation. Kept so a later attempt is told the
/// truth rather than "not issued", which would send someone looking for a typo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ForwardFate {
    Exhausted,
    Expired,
    Revoked,
}

const FORWARD_MEMORY: usize = 4096;

/// Authorised forwards, and the budget each one has.
///
/// A forward ticket differs from a session credential in exactly one way, and it
/// is a deliberate one: it is *budgeted* rather than one-time, because a listener
/// has to accept more than one connection and there is no way to hand each
/// connection its own credential ahead of time. Everything else is stricter, not
/// looser — it is bound to one target, it expires, and it cannot outlive the
/// session that asked for it.
pub struct ForwardVault {
    limits: ForwardLimits,
    live: HashMap<String, LiveForward>,
    /// So closing a session can retire the forwards it authorised.
    by_session: HashMap<String, Vec<String>>,
    retired: VecDeque<String>,
    retired_set: HashMap<String, ForwardFate>,
    now: Box<dyn Fn() -> Instant + Send + Sync>,
}

struct LiveForward {
    target: ForwardTarget,
    session_id: String,
    issued_at: Instant,
    /// This ticket's own lifetime, which is what was asked for clamped to the
    /// server's maximum — not the maximum itself.
    ttl: Duration,
    uses_left: usize,
}

impl ForwardVault {
    pub fn new(limits: ForwardLimits) -> Self {
        Self {
            limits,
            live: HashMap::new(),
            by_session: HashMap::new(),
            retired: VecDeque::new(),
            retired_set: HashMap::new(),
            now: Box::new(Instant::now),
        }
    }

    pub fn with_clock(mut self, now: impl Fn() -> Instant + Send + Sync + 'static) -> Self {
        self.now = Box::new(now);
        self
    }

    pub fn limits(&self) -> ForwardLimits {
        self.limits
    }

    /// Authorise a forward and say what was actually agreed.
    ///
    /// The caller asked for a lifetime and a connection budget; the numbers
    /// returned are the ones in force, so a client is never told "30 minutes" by
    /// a server that caps forwards at five.
    pub fn issue(
        &mut self,
        target: ForwardTarget,
        session_id: &str,
        ttl: Duration,
        max_uses: usize,
    ) -> (ForwardTicket, Duration, usize) {
        let ticket = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let granted_ttl = ttl.min(self.limits.max_ttl).max(Duration::from_millis(1));
        let granted_uses = max_uses.min(self.limits.max_uses).max(1);
        let issued_at = (self.now)();
        self.live.insert(
            ticket.clone(),
            LiveForward {
                target,
                session_id: session_id.to_string(),
                issued_at,
                ttl: granted_ttl,
                uses_left: granted_uses,
            },
        );
        self.by_session
            .entry(session_id.to_string())
            .or_default()
            .push(ticket.clone());
        (ForwardTicket(ticket), granted_ttl, granted_uses)
    }

    /// Spend one use of a ticket, and say where the connection may go.
    ///
    /// The target comes *out* of the vault rather than in from the caller: that is
    /// what makes a ticket unusable as a way to reach something it was not issued
    /// for, however the request is worded.
    pub fn spend(&mut self, ticket: &str) -> Result<(ForwardTarget, String), ForwardTicketError> {
        self.sweep_expired();
        let last_use = self
            .live
            .get_mut(ticket)
            .map(|entry| {
                entry.uses_left -= 1;
                entry.uses_left == 0
            })
            .unwrap_or(false);
        if let Some(entry) = self.live.get(ticket) {
            let spent = (entry.target.clone(), entry.session_id.clone());
            if last_use {
                self.drop_ticket(ticket, ForwardFate::Exhausted);
            }
            return Ok(spent);
        }
        match self.retired_set.get(ticket) {
            Some(ForwardFate::Exhausted) => Err(ForwardTicketError::Exhausted),
            Some(ForwardFate::Expired) => Err(ForwardTicketError::Expired),
            Some(ForwardFate::Revoked) => Err(ForwardTicketError::Revoked),
            None => Err(ForwardTicketError::Unknown),
        }
    }

    /// Retire every forward a session authorised. Called when that session ends,
    /// which is what makes a closed session take its port forward with it instead
    /// of leaving a way in behind.
    pub fn revoke_session(&mut self, session_id: &str) -> usize {
        let Some(tickets) = self.by_session.remove(session_id) else {
            return 0;
        };
        let mut count = 0;
        for ticket in tickets {
            if self.live.remove(&ticket).is_some() {
                self.retire(&ticket, ForwardFate::Revoked);
                count += 1;
            }
        }
        count
    }

    pub fn live_count(&self) -> usize {
        self.live.len()
    }

    /// Tickets whose lifetime has passed, whether or not anyone presented them.
    pub fn sweep_expired(&mut self) -> usize {
        let now = (self.now)();
        let stale: Vec<String> = self
            .live
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.issued_at) >= entry.ttl)
            .map(|(ticket, _)| ticket.clone())
            .collect();
        let count = stale.len();
        for ticket in stale {
            self.drop_ticket(&ticket, ForwardFate::Expired);
        }
        count
    }

    /// Take a ticket out of circulation and out of the by-session index, so the
    /// index does not outlive the thing it points at.
    fn drop_ticket(&mut self, ticket: &str, fate: ForwardFate) {
        if let Some(entry) = self.live.remove(ticket)
            && let Some(tickets) = self.by_session.get_mut(&entry.session_id)
        {
            tickets.retain(|live| live != ticket);
            if tickets.is_empty() {
                self.by_session.remove(&entry.session_id);
            }
        }
        self.retire(ticket, fate);
    }

    fn retire(&mut self, ticket: &str, fate: ForwardFate) {
        if self.retired_set.len() >= FORWARD_MEMORY
            && let Some(oldest) = self.retired.pop_front()
        {
            self.retired_set.remove(&oldest);
        }
        if self.retired_set.insert(ticket.to_string(), fate).is_none() {
            self.retired.push_back(ticket.to_string());
        }
    }
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

    fn forward_vault(ttl: Duration) -> (ForwardVault, FakeClock) {
        let clock = FakeClock::default();
        let probe = clock.clone();
        let vault = ForwardVault::new(ForwardLimits {
            max_ttl: ttl,
            max_uses: 8,
        })
        .with_clock(move || probe.instant());
        (vault, clock)
    }

    /// A listener has to accept more than one connection, so a forward ticket is
    /// budgeted rather than one-time. The budget is still a hard number.
    #[test]
    fn a_forward_ticket_opens_exactly_the_connections_it_buys() {
        let (mut vault, _clock) = forward_vault(Duration::from_secs(60));
        let (ticket, _, uses) = vault.issue(
            ForwardTarget::new("db.internal", 5432),
            "session-1",
            Duration::from_secs(60),
            3,
        );
        assert_eq!(uses, 3);
        for _ in 0..3 {
            assert_eq!(
                vault.spend(ticket.as_str()).unwrap().0,
                ForwardTarget::new("db.internal", 5432)
            );
        }
        assert_eq!(
            vault.spend(ticket.as_str()),
            Err(ForwardTicketError::Exhausted),
            "the fourth connection is the one the budget exists to stop"
        );
        assert_eq!(vault.live_count(), 0, "a spent ticket is not left behind");
    }

    /// The target comes out of the vault, so there is nothing for a request to
    /// point a ticket at.
    #[test]
    fn a_forward_ticket_reaches_only_what_it_was_issued_for() {
        let (mut vault, _clock) = forward_vault(Duration::from_secs(60));
        let (db, _, _) = vault.issue(
            ForwardTarget::new("db.internal", 5432),
            "session-1",
            Duration::from_secs(60),
            4,
        );
        let (metrics, _, _) = vault.issue(
            ForwardTarget::new("127.0.0.1", 9090),
            "session-1",
            Duration::from_secs(60),
            4,
        );
        assert_ne!(db, metrics);
        assert_eq!(
            vault.spend(db.as_str()).unwrap().0,
            ForwardTarget::new("db.internal", 5432)
        );
        assert_eq!(
            vault.spend(metrics.as_str()).unwrap().0,
            ForwardTarget::new("127.0.0.1", 9090)
        );
    }

    /// Closing the session is what closes the forward. Without this, a ticket
    /// outlives the tunnel it was minted for and the host keeps a way in.
    #[test]
    fn a_forward_ticket_dies_with_the_session_that_minted_it() {
        let (mut vault, _clock) = forward_vault(Duration::from_secs(60));
        let (mine, _, _) = vault.issue(
            ForwardTarget::new("127.0.0.1", 8080),
            "session-1",
            Duration::from_secs(60),
            8,
        );
        let (theirs, _, _) = vault.issue(
            ForwardTarget::new("127.0.0.1", 8080),
            "session-2",
            Duration::from_secs(60),
            8,
        );
        assert_eq!(vault.revoke_session("session-1"), 1);
        assert_eq!(vault.spend(mine.as_str()), Err(ForwardTicketError::Revoked));
        assert!(
            vault.spend(theirs.as_str()).is_ok(),
            "another session's forward is not yours to close"
        );
    }

    #[test]
    fn a_forward_ticket_expires_even_if_nobody_presents_it() {
        let (mut vault, clock) = forward_vault(Duration::from_millis(100));
        let (ticket, _, _) = vault.issue(
            ForwardTarget::new("127.0.0.1", 8080),
            "session-1",
            Duration::from_millis(100),
            8,
        );
        clock.advance(99);
        assert!(vault.spend(ticket.as_str()).is_ok());
        clock.advance(2);
        assert_eq!(vault.sweep_expired(), 1);
        assert_eq!(
            vault.spend(ticket.as_str()),
            Err(ForwardTicketError::Expired),
            "a tunnel nobody closed stops working on its own"
        );
    }

    /// The server decides how long and how many; a client's number is a request.
    #[test]
    fn a_grant_is_clamped_to_what_the_server_will_agree_to() {
        let (mut vault, _clock) = forward_vault(Duration::from_millis(500));
        let (_ticket, ttl, uses) = vault.issue(
            ForwardTarget::new("127.0.0.1", 8080),
            "session-1",
            Duration::from_secs(86_400),
            100_000,
        );
        assert_eq!(ttl, Duration::from_millis(500));
        assert_eq!(uses, 8);
        let (_zero, ttl, uses) = vault.issue(
            ForwardTarget::new("127.0.0.1", 8080),
            "session-1",
            Duration::ZERO,
            0,
        );
        assert!(ttl > Duration::ZERO && uses >= 1, "{ttl:?} / {uses}");
    }

    /// The two credential kinds live in separate stores on purpose: a session
    /// credential must not open a forward, and a forward ticket must not open a
    /// session.
    #[test]
    fn a_forward_ticket_is_not_a_session_credential_or_vice_versa() {
        let (mut sessions, _clock) = vault(Duration::from_secs(60));
        let (mut forwards, _clock) = forward_vault(Duration::from_secs(60));
        let session_token = sessions.issue();
        let (ticket, _, _) = forwards.issue(
            ForwardTarget::new("127.0.0.1", 8080),
            "session-1",
            Duration::from_secs(60),
            2,
        );
        assert_eq!(sessions.redeem(ticket.as_str()), Err(RedeemError::Unknown));
        assert_eq!(
            forwards.spend(session_token.as_str()),
            Err(ForwardTicketError::Unknown)
        );
    }

    #[test]
    fn forward_tickets_are_not_printed_and_targets_are() {
        let (mut vault, _clock) = forward_vault(Duration::from_secs(60));
        let (ticket, _, _) = vault.issue(
            ForwardTarget::new("db.internal", 5432),
            "session-1",
            Duration::from_secs(60),
            2,
        );
        let debug = format!("{ticket:?}");
        assert!(!debug.contains(ticket.as_str()), "{debug}");
        assert_eq!(
            ForwardTarget::new("db.internal", 5432).to_text(),
            "db.internal:5432"
        );
        assert_eq!(ForwardTarget::new("::1", 8080).to_text(), "[::1]:8080");
    }

    /// The index that lets a session revoke its own forwards must not outlive
    /// them, or a long session accumulates a list of dead tickets.
    #[test]
    fn a_spent_forward_leaves_nothing_indexed() {
        let (mut vault, _clock) = forward_vault(Duration::from_secs(60));
        let (ticket, _, _) = vault.issue(
            ForwardTarget::new("127.0.0.1", 1),
            "session-1",
            Duration::from_secs(60),
            1,
        );
        vault.spend(ticket.as_str()).unwrap();
        assert!(vault.by_session.is_empty(), "the index kept a dead ticket");
        assert_eq!(vault.revoke_session("session-1"), 0);
        assert_eq!(
            vault.spend(ticket.as_str()),
            Err(ForwardTicketError::Exhausted)
        );
    }
}
