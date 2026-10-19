//! First-run password setup and browser sessions for the web host.
//!
//! The official ZCode frontend has no login screen of its own, so an
//! unauthenticated navigation receives this small inline page from the host.
//! After login, an HttpOnly SameSite cookie accompanies both HTTP and WebSocket
//! requests; the password itself is never stored in browser storage or sent on
//! subsequent requests.

use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    net::{IpAddr, SocketAddr},
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{Html, IntoResponse, Response},
};
use ring::{
    pbkdf2,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PASSWORD_FORMAT_VERSION: u32 = 1;
const PASSWORD_ITERATIONS: u32 = 600_000;
const PASSWORD_SALT_LEN: usize = 16;
const PASSWORD_HASH_LEN: usize = 32;
const SESSION_TOKEN_LEN: usize = 32;
const SETUP_CODE_LEN: usize = 24;
const MAX_SESSIONS: usize = 2048;
const MIN_ACCEPTED_ITERATIONS: u32 = 100_000;
const MAX_ACCEPTED_ITERATIONS: u32 = 2_000_000;
const SESSION_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const SESSION_COOKIE: &str = "chaos_web_session";
const HTTP_SESSION_COOKIE: &str = "chaos_web_http_session";
const LOGIN_FAILURE_LIMIT: u32 = 5;
const LOGIN_LOCKOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
pub struct WebAuth {
    inner: Arc<AuthInner>,
}

struct AuthInner {
    enabled: bool,
    /// 路由层声明的文件根。在 `RwLock` 后面：`WebAuth` 可克隆，任何句柄都
    /// 能完成 `with_file_root` 注入，不存在穿透 `Arc` 赋值的问题。
    file_root: RwLock<PathBuf>,
    path: Option<PathBuf>,
    password: RwLock<Option<PasswordRecord>>,
    sessions: Mutex<HashMap<[u8; 32], u64>>,
    failures: Mutex<HashMap<IpAddr, LoginFailure>>,
    password_work: Arc<tokio::sync::Semaphore>,
    bootstrap_code: Mutex<Option<String>>,
    bootstrap_path: Option<PathBuf>,
}

#[derive(Clone)]
struct PasswordRecord {
    salt: [u8; PASSWORD_SALT_LEN],
    hash: [u8; PASSWORD_HASH_LEN],
    iterations: NonZeroU32,
}

#[derive(Default)]
struct LoginFailure {
    count: u32,
    blocked_until: Option<SystemTime>,
}

#[derive(Serialize, Deserialize)]
struct PasswordFile {
    version: u32,
    algorithm: String,
    iterations: u32,
    salt_hex: String,
    hash_hex: String,
}

#[derive(Deserialize)]
pub struct PasswordSubmission {
    pub password: String,
    #[serde(default)]
    pub confirmation: String,
    #[serde(default)]
    pub setup_code: String,
}

#[derive(Serialize)]
struct AuthStatus {
    setup_required: bool,
    setup_code_required: bool,
    authenticated: bool,
}

/// The filesystem root exposed by the web host. Override only by an operator
/// setting `CHAOS_WEB_FILE_ROOT`; otherwise default to the process working tree.
pub fn file_root() -> PathBuf {
    if let Some(declared) = std::env::var_os("CHAOS_WEB_FILE_ROOT") {
        let declared = PathBuf::from(declared);
        return dunce::canonicalize(&declared).unwrap_or(declared);
    }
    std::env::current_dir()
        .map(|dir| dunce::canonicalize(&dir).unwrap_or(dir))
        .unwrap_or_else(|_| PathBuf::from("/"))
}

/// Resolve the password-record location used by the shipped host.
pub fn password_file_path_from_env() -> Option<PathBuf> {
    std::env::var_os("CHAOS_WEB_AUTH_FILE")
        .map(PathBuf::from)
        .or_else(|| {
            chaos_engine::chaos_store::chaos_home().map(|home| home.join("web/auth/web_auth.json"))
        })
}

fn setup_code_path(password_path: &Path) -> PathBuf {
    password_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("web_auth.json.setup-code")
}

fn sessions_path(password_path: &Path) -> PathBuf {
    password_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("web_auth.sessions.json")
}

fn read_sessions_file(path: &Path) -> HashMap<[u8; 32], u64> {
    let now = unix_seconds();
    let mut map = HashMap::new();
    if let Ok(content) = fs::read_to_string(path)
        && let Ok(raw_map) = serde_json::from_str::<HashMap<String, u64>>(&content)
    {
        for (hex_key, expires) in raw_map {
            if expires > now {
                if let Some(key_bytes) = decode_hex::<32>(&hex_key) {
                    map.insert(key_bytes, expires);
                }
            }
        }
    }
    map
}

fn persist_sessions_file(path: &Path, sessions: &HashMap<[u8; 32], u64>) -> anyhow::Result<()> {
    let raw_map: HashMap<String, u64> = sessions.iter().map(|(k, v)| (encode_hex(k), *v)).collect();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    let json = serde_json::to_string(&raw_map)?;
    fs::write(&temp, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&temp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&temp, path)?;
    Ok(())
}

fn same_canonical_path(candidate: &Path, secret: &Path) -> bool {
    let candidate = dunce::canonicalize(candidate).unwrap_or_else(|_| candidate.to_path_buf());
    let secret = dunce::canonicalize(secret).unwrap_or_else(|_| secret.to_path_buf());
    candidate == secret
}

/// Whether a filesystem-channel path names any web-authentication secret.
pub fn is_auth_secret_path(candidate: &Path) -> bool {
    let Some(secret) = password_file_path_from_env() else {
        return false;
    };
    is_auth_secret_path_for(candidate, &secret)
}

pub fn is_auth_secret_path_for(candidate: &Path, password_file: &Path) -> bool {
    let setup_code = password_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("web_auth.json.setup-code");
    let sessions_file = password_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("web_auth.sessions.json");
    same_canonical_path(candidate, password_file)
        || same_canonical_path(candidate, &setup_code)
        || same_canonical_path(candidate, &sessions_file)
}

impl WebAuth {
    /// Production auth state. A missing file means the browser must complete
    /// first-run setup; an unreadable or malformed file fails closed at startup.
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        Self::open_with_file_root(path, file_root())
    }

    pub fn open_with_file_root(path: impl AsRef<Path>, file_root: PathBuf) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let password = read_password_file(&path)?;
        let bootstrap_path = setup_code_path(&path);
        let setup_code = if password.is_none() {
            Some(read_or_create_setup_code(&bootstrap_path)?)
        } else {
            None
        };
        if let Some(record) = &password {
            anyhow::ensure!(
                (MIN_ACCEPTED_ITERATIONS..=MAX_ACCEPTED_ITERATIONS)
                    .contains(&record.iterations.get()),
                "password hash iteration count is outside accepted bounds"
            );
        }
        let initial_sessions = read_sessions_file(&sessions_path(&path));
        Ok(Self {
            inner: Arc::new(AuthInner {
                enabled: true,
                file_root: RwLock::new(file_root),
                path: Some(path),
                password: RwLock::new(password),
                sessions: Mutex::new(initial_sessions),
                failures: Mutex::new(HashMap::new()),
                password_work: Arc::new(tokio::sync::Semaphore::new(4)),
                bootstrap_code: Mutex::new(setup_code),
                bootstrap_path: Some(bootstrap_path),
            }),
        })
    }

    /// Router constructors used by in-process protocol tests do not represent a
    /// deployed browser host. Production always passes [`Self::open`].
    pub fn disabled() -> Self {
        Self {
            inner: Arc::new(AuthInner {
                enabled: false,
                file_root: RwLock::new(file_root()),
                path: None,
                password: RwLock::new(None),
                sessions: Mutex::new(HashMap::new()),
                failures: Mutex::new(HashMap::new()),
                password_work: Arc::new(tokio::sync::Semaphore::new(4)),
                bootstrap_code: Mutex::new(None),
                bootstrap_path: None,
            }),
        }
    }

    pub fn enabled(&self) -> bool {
        self.inner.enabled
    }

    pub fn file_root(&self) -> PathBuf {
        self.inner
            .file_root
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// 注入路由层声明的文件根；`None` 保留原值。
    pub fn with_file_root(self, file_root: Option<PathBuf>) -> Self {
        if let Some(root) = file_root {
            *self
                .inner
                .file_root
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = root;
        }
        self
    }

    /// Private file containing the one-time code for first setup through a
    /// remote TLS proxy. The secret itself is never printed into general logs.
    pub fn bootstrap_code(&self) -> Option<String> {
        self.inner
            .bootstrap_code
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn bootstrap_code_file(&self) -> Option<PathBuf> {
        self.inner
            .bootstrap_code
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()?;
        self.inner.bootstrap_path.clone()
    }

    pub fn setup_required(&self) -> bool {
        self.inner.enabled
            && self
                .inner
                .password
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none()
    }

    pub async fn setup_async(
        &self,
        password: String,
        peer: IpAddr,
        setup_code: Option<String>,
    ) -> anyhow::Result<String> {
        let permit = Arc::clone(&self.inner.password_work)
            .acquire_owned()
            .await
            .map_err(|_| anyhow::anyhow!("password worker is unavailable"))?;
        let auth = self.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            auth.setup(&password, peer, setup_code.as_deref())
        })
        .await
        .map_err(|error| anyhow::anyhow!("password worker failed: {error}"))?
    }

    pub async fn login_async(&self, password: String, peer: IpAddr) -> anyhow::Result<String> {
        let permit = Arc::clone(&self.inner.password_work)
            .acquire_owned()
            .await
            .map_err(|_| anyhow::anyhow!("password worker is unavailable"))?;
        let auth = self.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            auth.login(&password, peer)
        })
        .await
        .map_err(|error| anyhow::anyhow!("password worker failed: {error}"))?
    }

    /// Set the first password exactly once, with an exclusive file create so a
    /// concurrent first-run request cannot replace the password another browser
    /// has just chosen.
    pub fn setup(
        &self,
        password: &str,
        peer: IpAddr,
        setup_code: Option<&str>,
    ) -> anyhow::Result<String> {
        anyhow::ensure!(self.inner.enabled, "browser password auth is disabled");
        self.check_lockout(peer)?;
        let bootstrap = self
            .inner
            .bootstrap_code
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(code) = setup_code {
            if !bootstrap
                .as_deref()
                .is_some_and(|expected| constant_time_text_equal(expected, code))
            {
                drop(bootstrap);
                self.record_failure(peer);
                anyhow::bail!("invalid first-run setup code");
            }
        } else {
            anyhow::ensure!(
                peer.is_loopback(),
                "first password setup requires localhost or the one-time setup code"
            );
        }
        drop(bootstrap);
        validate_password(password)?;
        let path = self
            .inner
            .path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing password storage path"))?;
        let mut stored = self
            .inner
            .password
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        anyhow::ensure!(stored.is_none(), "password is already configured");

        let record = PasswordRecord::new(password.as_bytes())?;
        persist_password_file(path, &record)?;
        *stored = Some(record);
        drop(stored);
        self.inner
            .bootstrap_code
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(path) = &self.inner.bootstrap_path {
            let _ = fs::remove_file(path);
        }
        self.inner
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&peer);
        Ok(self.issue_session()?)
    }

    pub fn login(&self, password: &str, peer: IpAddr) -> anyhow::Result<String> {
        anyhow::ensure!(self.inner.enabled, "browser password auth is disabled");
        anyhow::ensure!(!self.setup_required(), "password setup is required");
        self.check_lockout(peer)?;
        let verified = {
            let stored = self
                .inner
                .password
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            stored
                .as_ref()
                .is_some_and(|record| record.verify(password.as_bytes()))
        };
        if !verified {
            self.record_failure(peer);
            anyhow::bail!("invalid password");
        }
        self.inner
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&peer);
        self.issue_session()
    }

    pub fn authenticated(&self, headers: &HeaderMap) -> bool {
        if !self.inner.enabled {
            return true;
        }
        let token = cookie_value(headers, HTTP_SESSION_COOKIE)
            .or_else(|| cookie_value(headers, SESSION_COOKIE));
        let Some(token) = token else {
            return false;
        };
        let Some(raw) = decode_hex::<SESSION_TOKEN_LEN>(token) else {
            return false;
        };
        let digest: [u8; 32] = Sha256::digest(raw).into();
        let now = unix_seconds();
        let mut sessions = self
            .inner
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.retain(|_, expires| *expires > now);
        sessions.contains_key(&digest)
    }

    pub fn logout(&self, headers: &HeaderMap) {
        let token = cookie_value(headers, HTTP_SESSION_COOKIE)
            .or_else(|| cookie_value(headers, SESSION_COOKIE));
        if let Some(token) = token
            && let Some(raw) = decode_hex::<SESSION_TOKEN_LEN>(token)
        {
            let digest: [u8; 32] = Sha256::digest(raw).into();
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if sessions.remove(&digest).is_some()
                && let Some(path) = &self.inner.path
            {
                let _ = persist_sessions_file(&sessions_path(path), &sessions);
            }
        }
    }

    fn issue_session(&self) -> anyhow::Result<String> {
        let mut token = [0_u8; SESSION_TOKEN_LEN];
        SystemRandom::new()
            .fill(&mut token)
            .map_err(|_| anyhow::anyhow!("secure random source unavailable"))?;
        let expires = unix_seconds().saturating_add(SESSION_MAX_AGE.as_secs());
        let mut sessions = self
            .inner
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = unix_seconds();
        sessions.retain(|_, expiry| *expiry > now);
        if sessions.len() >= MAX_SESSIONS {
            if let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, expiry)| *expiry)
                .map(|(token, _)| *token)
            {
                sessions.remove(&oldest);
            }
        }
        sessions.insert(Sha256::digest(token).into(), expires);
        if let Some(path) = &self.inner.path {
            let _ = persist_sessions_file(&sessions_path(path), &sessions);
        }
        Ok(encode_hex(&token))
    }

    fn check_lockout(&self, peer: IpAddr) -> anyhow::Result<()> {
        let failures = self
            .inner
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if failures
            .get(&peer)
            .and_then(|state| state.blocked_until)
            .is_some_and(|until| until > SystemTime::now())
        {
            anyhow::bail!("too many failed login attempts; try again later");
        }
        Ok(())
    }

    fn record_failure(&self, peer: IpAddr) {
        let mut failures = self
            .inner
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = failures.entry(peer).or_default();
        state.count = state.count.saturating_add(1);
        if state.count >= LOGIN_FAILURE_LIMIT {
            state.blocked_until = SystemTime::now().checked_add(LOGIN_LOCKOUT);
            state.count = 0;
        }
    }
}

impl PasswordRecord {
    fn new(password: &[u8]) -> anyhow::Result<Self> {
        let mut salt = [0_u8; PASSWORD_SALT_LEN];
        SystemRandom::new()
            .fill(&mut salt)
            .map_err(|_| anyhow::anyhow!("secure random source unavailable"))?;
        let iterations = NonZeroU32::new(PASSWORD_ITERATIONS)
            .ok_or_else(|| anyhow::anyhow!("invalid PBKDF2 iteration count"))?;
        let mut hash = [0_u8; PASSWORD_HASH_LEN];
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            &salt,
            password,
            &mut hash,
        );
        Ok(Self {
            salt,
            hash,
            iterations,
        })
    }

    fn verify(&self, password: &[u8]) -> bool {
        pbkdf2::verify(
            pbkdf2::PBKDF2_HMAC_SHA256,
            self.iterations,
            &self.salt,
            password,
            &self.hash,
        )
        .is_ok()
    }
}

fn read_or_create_setup_code(path: &Path) -> anyhow::Result<String> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("setup-code path has no parent"))?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.file_type().is_file(),
                "setup-code path must be a regular file"
            );
            anyhow::ensure!(metadata.len() <= 128, "setup-code file is too large");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                anyhow::ensure!(
                    metadata.permissions().mode() & 0o077 == 0,
                    "setup-code file must be private (chmod 600)"
                );
            }
            let code = fs::read_to_string(path)?.trim().to_owned();
            anyhow::ensure!(
                decode_hex::<SETUP_CODE_LEN>(&code).is_some(),
                "invalid setup-code file"
            );
            Ok(code)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let code = generate_setup_code()?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(path)?;
            file.write_all(code.as_bytes())?;
            file.sync_all()?;
            Ok(code)
        }
        Err(error) => Err(error.into()),
    }
}

fn generate_setup_code() -> anyhow::Result<String> {
    let mut bytes = [0_u8; SETUP_CODE_LEN];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("secure random source unavailable"))?;
    Ok(encode_hex(&bytes))
}

fn constant_time_text_equal(expected: &str, candidate: &str) -> bool {
    let mut difference = expected.len() ^ candidate.len();
    for (left, right) in expected.as_bytes().iter().zip(candidate.as_bytes()) {
        difference |= usize::from(left != right);
    }
    difference == 0
}

fn validate_password(password: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        password.chars().count() >= 12,
        "password must be at least 12 characters"
    );
    anyhow::ensure!(password.len() <= 1024, "password is too long");
    anyhow::ensure!(
        password.bytes().all(|byte| !byte.is_ascii_control()),
        "password must not contain control characters"
    );
    Ok(())
}

fn read_password_file(path: &Path) -> anyhow::Result<Option<PasswordRecord>> {
    let metadata = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(metadata) => metadata,
    };
    anyhow::ensure!(
        metadata.file_type().is_file(),
        "web password file must be a regular file, not a symlink"
    );
    anyhow::ensure!(metadata.len() <= 4096, "web password file is too large");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        anyhow::ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "web password file must not be accessible by group or other users (chmod 600)"
        );
    }
    let file: PasswordFile = serde_json::from_slice(&fs::read(path)?)?;
    anyhow::ensure!(
        file.version == PASSWORD_FORMAT_VERSION,
        "unsupported web password file version"
    );
    anyhow::ensure!(
        file.algorithm == "pbkdf2-hmac-sha256",
        "unsupported password hash algorithm"
    );
    let iterations = NonZeroU32::new(file.iterations)
        .ok_or_else(|| anyhow::anyhow!("invalid password hash iteration count"))?;
    let salt = decode_hex::<PASSWORD_SALT_LEN>(&file.salt_hex)
        .ok_or_else(|| anyhow::anyhow!("invalid password salt"))?;
    let hash = decode_hex::<PASSWORD_HASH_LEN>(&file.hash_hex)
        .ok_or_else(|| anyhow::anyhow!("invalid password digest"))?;
    Ok(Some(PasswordRecord {
        salt,
        hash,
        iterations,
    }))
}

fn persist_password_file(path: &Path, record: &PasswordRecord) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("password path has no parent"))?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    let file = PasswordFile {
        version: PASSWORD_FORMAT_VERSION,
        algorithm: "pbkdf2-hmac-sha256".into(),
        iterations: record.iterations.get(),
        salt_hex: encode_hex(&record.salt),
        hash_hex: encode_hex(&record.hash),
    };
    let bytes = serde_json::to_vec(&file)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut handle = options.open(path)?;
    if let Err(error) = handle.write_all(&bytes).and_then(|()| handle.sync_all()) {
        drop(handle);
        let _ = fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}

fn cookie_value<'a>(headers: &'a HeaderMap, key: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(name, value)| (name == key).then_some(value))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 {
        return None;
    }
    let mut output = [0_u8; N];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let digit = |byte: u8| -> Option<u8> {
            match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            }
        };
        output[index] = digit(pair[0])?
            .checked_mul(16)?
            .checked_add(digit(pair[1])?)?;
    }
    Some(output)
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn cookie_headers(token: &str, headers: &HeaderMap) -> Vec<HeaderValue> {
    let secure = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("https"));
    let mut values = Vec::new();
    if secure {
        if let Ok(v) = HeaderValue::from_str(&format!(
            "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}; Secure",
            SESSION_MAX_AGE.as_secs()
        )) {
            values.push(v);
        }
    } else {
        if let Ok(v) = HeaderValue::from_str(&format!(
            "{HTTP_SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
            SESSION_MAX_AGE.as_secs()
        )) {
            values.push(v);
        }
        if let Ok(v) = HeaderValue::from_str(&format!(
            "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
            SESSION_MAX_AGE.as_secs()
        )) {
            values.push(v);
        }
    }
    values
}

fn clear_cookie_headers(headers: &HeaderMap) -> Vec<HeaderValue> {
    let secure = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("https"));
    let mut values = Vec::new();
    let sec_attr = if secure { "; Secure" } else { "" };
    if let Ok(v) = HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{sec_attr}"
    )) {
        values.push(v);
    }
    if let Ok(v) = HeaderValue::from_str(&format!(
        "{HTTP_SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"
    )) {
        values.push(v);
    }
    values
}

fn auth_response_error(status: StatusCode, message: &'static str) -> Response {
    let mut response = (status, Json(serde_json::json!({ "error": message }))).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'self'; frame-ancestors 'none'"),
    );
    response
}

fn request_origin_allowed(headers: &HeaderMap) -> bool {
    super::host_allowed(headers) && super::origin_allowed(headers)
}

/// The local TLS proxy overwrites X-Real-IP with the connecting client address.
/// Trust it only when the TCP peer is loopback; a remote peer cannot choose its
/// own rate-limit bucket by injecting this header.
fn client_ip(headers: &HeaderMap, peer: IpAddr) -> IpAddr {
    if peer.is_loopback()
        && let Some(real) = headers
            .get("x-real-ip")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<IpAddr>().ok())
    {
        return real;
    }
    peer
}

fn login_transport_allowed(headers: &HeaderMap, peer: IpAddr) -> bool {
    let _ = peer;
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    let Ok(uri) = origin.parse::<axum::http::Uri>() else {
        return false;
    };
    if uri.authority().is_some_and(super::loopback_authority) {
        return uri.scheme_str() == Some("http") || uri.scheme_str() == Some("https");
    }
    if headers.contains_key("x-forwarded-proto") {
        return uri.scheme_str() == Some("https") && super::forwarded_proto_https(headers);
    }
    uri.scheme_str() == Some("http") || uri.scheme_str() == Some("https")
}

fn local_direct_origin(headers: &HeaderMap) -> bool {
    let host_is_loopback = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|host| host.parse::<axum::http::uri::Authority>().ok())
        .is_some_and(|authority| super::loopback_authority(&authority));
    let origin_is_loopback = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|origin| {
            origin.parse::<axum::http::Uri>().ok().is_some_and(|uri| {
                uri.authority().is_some_and(super::loopback_authority)
                    && matches!(uri.scheme_str(), Some("http" | "https"))
            })
        });
    host_is_loopback && origin_is_loopback
}

fn first_setup_allowed(headers: &HeaderMap, peer: IpAddr, has_setup_code: bool) -> bool {
    if !peer.is_loopback() {
        return false;
    }
    let remote_origin_is_tls = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .and_then(|origin| origin.parse::<axum::http::Uri>().ok())
        .is_some_and(|uri| uri.scheme_str() == Some("https"))
        && super::forwarded_proto_https(headers);
    (local_direct_origin(headers) || (has_setup_code && remote_origin_is_tls))
        && login_transport_allowed(headers, peer)
}

pub async fn status(
    State(state): State<Arc<super::WebState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if !request_origin_allowed(&headers) {
        return auth_response_error(StatusCode::UNAUTHORIZED, "host_or_origin_not_allowed");
    }
    if !login_transport_allowed(&headers, peer.ip()) {
        return auth_response_error(StatusCode::FORBIDDEN, "remote_access_requires_https");
    }
    let status = AuthStatus {
        setup_required: state.auth.setup_required(),
        setup_code_required: state.auth.setup_required() && !local_direct_origin(&headers),
        authenticated: state.auth.authenticated(&headers),
    };
    let mut response = Json(status).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[axum::debug_handler]
pub async fn setup(
    State(state): State<Arc<super::WebState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(input): Json<PasswordSubmission>,
) -> Response {
    if !request_origin_allowed(&headers) {
        return auth_response_error(StatusCode::UNAUTHORIZED, "host_or_origin_not_allowed");
    }
    // First-run setup is local-only unless the request uses the declared HTTPS
    // proxy and proves possession of the persisted one-time setup code.
    let setup_code = (!local_direct_origin(&headers)).then_some(input.setup_code.as_str());
    if !first_setup_allowed(
        &headers,
        peer.ip(),
        setup_code.is_some_and(|code| !code.is_empty()),
    ) {
        return auth_response_error(
            StatusCode::FORBIDDEN,
            "password_setup_requires_loopback_or_setup_code",
        );
    }
    if input.password != input.confirmation {
        return auth_response_error(StatusCode::BAD_REQUEST, "password_confirmation_mismatch");
    }
    match state
        .auth
        .setup_async(
            input.password,
            client_ip(&headers, peer.ip()),
            setup_code.map(str::to_owned),
        )
        .await
    {
        Ok(token) => {
            let mut response = Json(serde_json::json!({ "ok": true })).into_response();
            for cookie in cookie_headers(&token, &headers) {
                response.headers_mut().append(header::SET_COOKIE, cookie);
            }
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Err(error) if state.auth.setup_required() => {
            let message = if error.to_string().contains("at least 12") {
                "password_too_short"
            } else if error.to_string().contains("too long") {
                "password_too_long"
            } else if error.to_string().contains("control") {
                "password_contains_control_character"
            } else if error.to_string().contains("invalid first-run setup code") {
                "setup_code_invalid"
            } else if error.to_string().contains("setup code") {
                "setup_code_required"
            } else if error.to_string().contains("try again later") {
                "login_rate_limited"
            } else {
                "password_setup_failed"
            };
            auth_response_error(StatusCode::BAD_REQUEST, message)
        }
        Err(_) => auth_response_error(StatusCode::CONFLICT, "password_already_configured"),
    }
}

#[axum::debug_handler]
pub async fn login(
    State(state): State<Arc<super::WebState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(input): Json<PasswordSubmission>,
) -> Response {
    if !request_origin_allowed(&headers) {
        eprintln!(
            "[web-auth] 登录被拒 (host/origin not allowed): peer={}, host={:?}, origin={:?}",
            peer,
            headers.get(header::HOST),
            headers.get(header::ORIGIN)
        );
        return auth_response_error(StatusCode::UNAUTHORIZED, "host_or_origin_not_allowed");
    }
    if !login_transport_allowed(&headers, peer.ip()) {
        eprintln!(
            "[web-auth] 登录被拒 (transport not allowed): peer={}, x-forwarded-proto={:?}",
            peer,
            headers.get("x-forwarded-proto")
        );
        return auth_response_error(StatusCode::FORBIDDEN, "login_requires_https_off_loopback");
    }
    match state
        .auth
        .login_async(input.password, client_ip(&headers, peer.ip()))
        .await
    {
        Ok(token) => {
            eprintln!("[web-auth] 登录成功: peer={}", peer);
            let mut response = Json(serde_json::json!({ "ok": true })).into_response();
            for cookie in cookie_headers(&token, &headers) {
                response.headers_mut().append(header::SET_COOKIE, cookie);
            }
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Err(error) => {
            eprintln!("[web-auth] 登录失败: peer={}, error={}", peer, error);
            if error.to_string().contains("try again later") {
                auth_response_error(StatusCode::TOO_MANY_REQUESTS, "login_rate_limited")
            } else {
                auth_response_error(StatusCode::UNAUTHORIZED, "invalid_password")
            }
        }
    }
}

pub async fn logout(State(state): State<Arc<super::WebState>>, headers: HeaderMap) -> Response {
    if !request_origin_allowed(&headers) {
        return auth_response_error(StatusCode::UNAUTHORIZED, "host_or_origin_not_allowed");
    }
    state.auth.logout(&headers);
    let mut response = Json(serde_json::json!({ "ok": true })).into_response();
    for cookie in clear_cookie_headers(&headers) {
        response.headers_mut().append(header::SET_COOKIE, cookie);
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Static assets are served only with a session cookie; an unauthenticated
/// navigation receives an inline first-run/login page which requires no assets.
pub async fn gate(
    State(state): State<Arc<super::WebState>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if !state.auth.enabled()
        || request.uri().path() == "/health"
        || request.uri().path().starts_with("/api/auth/")
    {
        return next.run(request).await;
    }
    if !super::host_allowed(request.headers()) || !super::origin_allowed(request.headers()) {
        return auth_response_error(StatusCode::UNAUTHORIZED, "host_or_origin_not_allowed");
    }
    // The browser password and session cookie are credentials.
    // When behind an HTTP reverse proxy, require that the proxy terminated TLS (X-Forwarded-Proto: https).
    if request.headers().contains_key("x-forwarded-proto")
        && !super::forwarded_proto_https(request.headers())
    {
        return auth_response_error(
            StatusCode::FORBIDDEN,
            "remote_access_requires_local_tls_proxy",
        );
    }
    let path = request.uri().path().to_owned();
    let api_or_socket = path.starts_with("/api/") || path == "/ws" || path == "/ws/zcode";
    if state.auth.authenticated(request.headers())
        || (!state.token.is_empty() && super::authorized(&state, request.headers()))
    {
        return next.run(request).await;
    }
    let accepts_html = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/html"));
    let page_navigation =
        !api_or_socket && !path.starts_with("/assets/") && (path == "/" || accepts_html);
    if !page_navigation {
        return auth_response_error(StatusCode::UNAUTHORIZED, "credential_required");
    }
    let mut response = Html(LOGIN_HTML).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; form-action 'self'"));
    response
}

const LOGIN_HTML: &str = r##"<!doctype html>
<html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Chaos Web 登录</title>
<style>
:root{color-scheme:dark;font:16px system-ui,sans-serif;background:#171717;color:#eee}*{box-sizing:border-box}body{min-height:100vh;margin:0;display:grid;place-items:center;padding:24px}.card{width:min(420px,100%);padding:32px;border:1px solid #444;border-radius:16px;background:#242424;box-shadow:0 20px 60px #0008}h1{font-size:22px;margin:0 0 8px}p{color:#aaa;line-height:1.5;margin:0 0 24px}label{display:block;margin:16px 0 8px;color:#ccc;font-size:14px}input{width:100%;height:44px;padding:0 12px;border:1px solid #555;border-radius:8px;background:#161616;color:#fff;font:inherit}button{width:100%;height:44px;margin-top:22px;border:0;border-radius:8px;background:#cfcfcf;color:#171717;font-weight:700;font:inherit;cursor:pointer}button:disabled{opacity:.55;cursor:wait}.error{min-height:24px;color:#ff8585;margin-top:14px;font-size:14px}.hint{font-size:13px;color:#999;margin-top:18px}
</style><body><main class="card"><h1 id="title">Chaos Web</h1><p id="subtitle">检查登录状态…</p><form id="form" hidden><div id="setup-code-wrap" hidden><label for="setup-code">远程首次设置码</label><input id="setup-code" type="password" autocomplete="one-time-code"><div class="hint">仅从服务器本机可读取的 `web_auth.json.setup-code` 文件中复制。</div></div><label for="password">密钥</label><input id="password" type="password" autocomplete="current-password" minlength="12" required><div id="confirm-wrap" hidden><label for="confirmation">再次输入密钥</label><input id="confirmation" type="password" autocomplete="new-password" minlength="12"></div><button id="submit" type="submit">登录</button><div class="error" id="error" role="alert"></div><div class="hint" id="hint"></div></form></main>
<script>
(async()=>{const $=id=>document.getElementById(id),form=$('form'),p=$('password'),c=$('confirmation'),sc=$('setup-code'),status=await fetch('/api/auth/status',{cache:'no-store'}).then(r=>r.json()).catch(()=>null);if(!status){$('subtitle').textContent='无法连接服务，请检查服务器状态。';return}if(status.authenticated){location.reload();return}const setup=status.setup_required;$('title').textContent=setup?'首次设置密钥':'登录';$('subtitle').textContent=setup?'首次使用，请设置至少 12 个字符的登录密钥。':'请输入登录密钥以继续。';$('confirm-wrap').hidden=!setup;$('setup-code-wrap').hidden=!setup||!status.setup_code_required;c.required=setup;p.autocomplete=setup?'new-password':'current-password';$('submit').textContent=setup?'设置密钥并进入':'登录';$('hint').textContent=setup?(status.setup_code_required?'远程设置需从服务器终端读取 web_auth.json.setup-code 一次性码。':'本机 localhost 连接可直接设置。'):'忘记密钥？停止服务后删除 ~/.chaos/web/auth/web_auth.json，再从本机重新设置。';form.hidden=false;form.addEventListener('submit',async e=>{e.preventDefault();$('error').textContent='';if(setup&&p.value!==c.value){$('error').textContent='两次输入不一致。';return}const b=$('submit');b.disabled=true;try{const r=await fetch(setup?'/api/auth/setup':'/api/auth/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({password:p.value,confirmation:c.value,setup_code:sc.value})});if(!r.ok){const data=await r.json().catch(()=>({}));const messages={password_too_short:'密钥至少需要 12 个字符。',password_too_long:'密钥过长。',password_confirmation_mismatch:'两次输入不一致。',password_setup_requires_loopback_or_setup_code:'首次设置请从 localhost 访问，或提供有效的一次性设置码。',setup_code_required:'远程首次设置需要 web_auth.json.setup-code 中的一次性设置码。',setup_code_invalid:'一次性设置码不正确。',password_already_configured:'密钥已被其他请求设置，请刷新后登录。',login_rate_limited:'尝试次数过多，请 5 分钟后再试。',invalid_password:'密钥不正确。',credential_required:'登录状态已失效，请重新登录。',host_or_origin_not_allowed:'访问来源或域名不被允许，请核对访问 URL。',login_requires_https_off_loopback:'登录要求通过安全协议访问。'};$('error').textContent=messages[data.error]||'操作失败，请重试。';return}location.reload()}catch(_){$('error').textContent='无法连接服务，请稍后重试。'}finally{b.disabled=false}})})()
</script></body></html>"##;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn auth_hash_and_setup_code_are_not_exposed_through_the_file_channel() {
        let root = tempdir().unwrap();
        let password_file = root.path().join("auth/web_auth.json");
        let setup_code = root.path().join("auth/web_auth.json.setup-code");
        let normal_file = root.path().join("auth/notes.txt");
        assert!(is_auth_secret_path_for(&password_file, &password_file));
        assert!(is_auth_secret_path_for(&setup_code, &password_file));
        assert!(!is_auth_secret_path_for(&normal_file, &password_file));
    }

    #[test]
    fn password_file_is_private_and_password_hashes_are_not_reversible() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("web_auth.json");
        let auth = WebAuth::open(&path).unwrap();
        assert!(auth.setup_required());
        assert!(
            auth.setup("too-short", IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), None)
                .is_err()
        );
        let token = auth
            .setup(
                "a-long-first-run-password",
                IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
                None,
            )
            .unwrap();
        assert!(!auth.setup_required());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let stored = fs::read_to_string(&path).unwrap();
        assert!(!stored.contains("a-long-first-run-password"));
        let reopened = WebAuth::open(&path).unwrap();
        assert!(
            reopened
                .login(
                    "a-long-first-run-password",
                    IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
                )
                .is_ok()
        );
        assert!(
            reopened
                .login("wrong-password", IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
                .is_err()
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{SESSION_COOKIE}={token}")).unwrap(),
        );
        assert!(auth.authenticated(&headers));
        auth.logout(&headers);
        assert!(!auth.authenticated(&headers));
    }

    #[test]
    fn a_remote_first_setup_requires_the_one_time_bootstrap_code() {
        let dir = tempdir().unwrap();
        let auth = WebAuth::open(dir.path().join("web_auth.json")).unwrap();
        let code = auth.bootstrap_code().expect("initial setup code");
        assert_eq!(code.len(), SETUP_CODE_LEN * 2);
        assert!(
            auth.setup(
                "first-password-is-long-enough",
                IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
                Some("wrong")
            )
            .is_err()
        );
        assert!(
            auth.setup_required(),
            "wrong code must not claim the password"
        );
        auth.setup(
            "first-password-is-long-enough",
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            Some(&code),
        )
        .expect("correct one-time code");
        assert!(!auth.setup_required());
        assert!(auth.bootstrap_code().is_none(), "one-time code is consumed");
    }

    #[test]
    fn first_run_password_file_cannot_be_overwritten() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("web_auth.json");
        let auth = WebAuth::open(&path).unwrap();
        auth.setup(
            "first-password-is-long-enough",
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            None,
        )
        .unwrap();
        assert!(
            auth.setup(
                "second-password-is-long-enough",
                IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
                None
            )
            .is_err()
        );
        assert!(
            WebAuth::open(&path)
                .unwrap()
                .login(
                    "first-password-is-long-enough",
                    IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
                )
                .is_ok()
        );
    }

    #[test]
    fn only_a_loopback_proxy_may_supply_the_login_rate_limit_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("192.168.1.20"));
        assert_eq!(
            client_ip(&headers, IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
            IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 20))
        );
        assert_eq!(
            client_ip(
                &headers,
                IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 8))
            ),
            IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 8)),
            "a remote peer cannot pick its own throttle bucket"
        );
    }

    #[test]
    fn first_run_setup_requires_a_loopback_peer_host_and_origin() {
        use axum::http::HeaderValue;
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8787"));
        assert!(first_setup_allowed(
            &headers,
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            false
        ));
        assert!(!first_setup_allowed(
            &headers,
            IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 4)),
            false
        ));
        headers.insert(header::HOST, HeaderValue::from_static("192.168.1.2:8787"));
        assert!(!first_setup_allowed(
            &headers,
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            true
        ));
    }

    #[test]
    fn a_remote_tls_proxy_needs_the_bootstrap_code_but_plaintext_is_refused() {
        use axum::http::HeaderValue;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::HOST,
            HeaderValue::from_static("zcode.example.test:3082"),
        );
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://zcode.example.test:3082"),
        );
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        let loopback = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        assert!(!first_setup_allowed(&headers, loopback, false));
        assert!(first_setup_allowed(&headers, loopback, true));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://zcode.example.test:3082"),
        );
        headers.insert("x-forwarded-proto", HeaderValue::from_static("http"));
        assert!(!first_setup_allowed(&headers, loopback, true));
        assert!(!login_transport_allowed(
            &headers,
            IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 9))
        ));
    }

    #[test]
    fn repeated_login_failures_lock_out_that_peer_temporarily() {
        let dir = tempdir().unwrap();
        let auth = WebAuth::open(dir.path().join("web_auth.json")).unwrap();
        auth.setup(
            "correct-password-long-enough",
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            None,
        )
        .unwrap();
        let peer = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        for _ in 0..LOGIN_FAILURE_LIMIT {
            assert!(auth.login("wrong-password-long-enough", peer).is_err());
        }
        assert!(
            auth.login("correct-password-long-enough", peer)
                .unwrap_err()
                .to_string()
                .contains("try again later")
        );
    }
}
