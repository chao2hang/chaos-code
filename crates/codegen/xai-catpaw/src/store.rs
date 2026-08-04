//! Encrypted SQLite account storage.
//!
//! Token columns always contain AES-256-GCM ciphertext. The 32-byte key lives
//! in a separate owner-only key file; there is deliberately no plaintext or
//! environment-variable fallback.

#[cfg(unix)]
use std::fs::{self, OpenOptions};
#[cfg(unix)]
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
#[cfg(unix)]
use rusqlite::OpenFlags;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::tokens::TokenSet;
use crate::{Error, Result};

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const CIPHERTEXT_MAGIC: &[u8] = b"CPAW1";
const ACCESS_AAD: &[u8] = b"xai-catpaw/account/access/v1";
const REFRESH_AAD: &[u8] = b"xai-catpaw/account/refresh/v1";

#[derive(Clone)]
struct SecretCipher {
    key: Arc<SecretKey>,
}

struct SecretKey([u8; KEY_LEN]);

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl SecretCipher {
    fn new(key: [u8; KEY_LEN]) -> Self {
        Self {
            key: Arc::new(SecretKey(key)),
        }
    }

    fn key(&self) -> Result<LessSafeKey> {
        let key = UnboundKey::new(&AES_256_GCM, &self.key.0)
            .map_err(|_| Error::Crypto("invalid AES-256-GCM key".into()))?;
        Ok(LessSafeKey::new(key))
    }

    fn encrypt(&self, aad: &'static [u8], plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0_u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Crypto("secure random nonce generation failed".into()))?;
        let mut sealed = plaintext.to_vec();
        self.key()?
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(aad),
                &mut sealed,
            )
            .map_err(|_| Error::Crypto("AES-256-GCM encryption failed".into()))?;
        let mut output = Vec::with_capacity(CIPHERTEXT_MAGIC.len() + NONCE_LEN + sealed.len());
        output.extend_from_slice(CIPHERTEXT_MAGIC);
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&sealed);
        Ok(output)
    }

    fn decrypt(&self, aad: &'static [u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
        let minimum = CIPHERTEXT_MAGIC.len() + NONCE_LEN + TAG_LEN;
        if ciphertext.len() < minimum || !ciphertext.starts_with(CIPHERTEXT_MAGIC) {
            return Err(Error::Crypto(
                "account token is not a supported encrypted value".into(),
            ));
        }
        let nonce_start = CIPHERTEXT_MAGIC.len();
        let nonce_end = nonce_start + NONCE_LEN;
        let nonce: [u8; NONCE_LEN] = ciphertext[nonce_start..nonce_end]
            .try_into()
            .expect("validated nonce length");
        let mut sealed = ciphertext[nonce_end..].to_vec();
        let plaintext = self
            .key()?
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(aad),
                &mut sealed,
            )
            .map_err(|_| Error::Crypto("AES-256-GCM authentication failed".into()))?;
        Ok(plaintext.to_vec())
    }
}

#[derive(Clone)]
pub struct AccountStore {
    connection: Arc<Mutex<Connection>>,
    cipher: SecretCipher,
    db_path: PathBuf,
    key_path: PathBuf,
}

impl std::fmt::Debug for AccountStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountStore")
            .field("db_path", &self.db_path)
            .field("key_path", &self.key_path)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct Account {
    pub id: i64,
    pub label: String,
    pub mobile: Option<String>,
    pub tokens: TokenSet,
    pub status: String,
    pub total_requests: i64,
    pub last_used_at: Option<i64>,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Account")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("mobile", &self.mobile)
            .field("tokens", &"[REDACTED]")
            .field("status", &self.status)
            .field("total_requests", &self.total_requests)
            .field("last_used_at", &self.last_used_at)
            .finish()
    }
}

struct RawAccount {
    id: i64,
    label: String,
    mobile: Option<String>,
    access_token: Vec<u8>,
    refresh_token: Vec<u8>,
    access_expires: i64,
    refresh_expires: i64,
    mis_id: Option<String>,
    status: String,
    total_requests: i64,
    last_used_at: Option<i64>,
}

const SELECT_COLUMNS: &str = "id, label, mobile, access_token, refresh_token, access_expires, refresh_expires, mis_id, status, total_requests, last_used_at";

impl AccountStore {
    /// Open or create an encrypted account database and its independent key.
    ///
    /// Existing files with group/world permission bits are rejected before the
    /// key or database is read. New files are created with mode `0600` on Unix.
    /// Non-Unix platforms currently fail closed rather than create files whose
    /// owner-only ACL cannot be guaranteed by this implementation.
    pub fn open(db_path: impl AsRef<Path>, key_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let key_path = key_path.as_ref().to_path_buf();
        if db_path == key_path {
            return Err(Error::Config(
                "account database and key must be separate files".into(),
            ));
        }
        #[cfg(not(unix))]
        return Err(Error::Config(
            "owner-only AccountStore files are currently supported only on Unix".into(),
        ));

        #[cfg(unix)]
        {
            prepare_parent(&db_path)?;
            prepare_parent(&key_path)?;
            let db_exists = path_entry_exists(&db_path)?;
            let key_exists = path_entry_exists(&key_path)?;
            // Validate every existing path before reading either one. In
            // particular, reject symlinks, foreign owners, and group/world
            // permissions even when its paired file is missing.
            if db_exists {
                validate_private_file(&db_path)?;
            }
            if key_exists {
                validate_private_file(&key_path)?;
            }
            if db_exists != key_exists {
                return Err(Error::Config(
                    "account database and key must either both exist or both be new".into(),
                ));
            }
            let key = load_or_create_key(&key_path)?;
            create_or_validate_private_file(&db_path)?;
            let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW;
            let connection = Connection::open_with_flags(&db_path, flags)?;
            connection.execute_batch(
                r#"
                PRAGMA journal_mode = DELETE;
                PRAGMA synchronous = FULL;
                CREATE TABLE IF NOT EXISTS accounts (
                    id              INTEGER PRIMARY KEY AUTOINCREMENT,
                    label           TEXT NOT NULL,
                    mobile          TEXT,
                    access_token    BLOB NOT NULL,
                    refresh_token   BLOB NOT NULL,
                    access_expires  INTEGER NOT NULL,
                    refresh_expires INTEGER NOT NULL,
                    mis_id          TEXT,
                    status          TEXT NOT NULL DEFAULT 'active',
                    total_requests  INTEGER NOT NULL DEFAULT 0,
                    last_used_at    INTEGER,
                    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_accounts_lru
                    ON accounts(status, last_used_at, id);
                "#,
            )?;
            validate_private_file(&db_path)?;
            Ok(Self {
                connection: Arc::new(Mutex::new(connection)),
                cipher: SecretCipher::new(key),
                db_path,
                key_path,
            })
        }
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn key_path(&self) -> &Path {
        &self.key_path
    }

    pub fn insert(&self, label: &str, mobile: Option<&str>, tokens: &TokenSet) -> Result<i64> {
        let access = self
            .cipher
            .encrypt(ACCESS_AAD, tokens.access_token.as_bytes())?;
        let refresh = self
            .cipher
            .encrypt(REFRESH_AAD, tokens.refresh_token.as_bytes())?;
        let connection = self.connection.lock();
        connection.execute(
            "INSERT INTO accounts
             (label, mobile, access_token, refresh_token, access_expires, refresh_expires, mis_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                label,
                mobile,
                access,
                refresh,
                tokens.expires,
                tokens.refresh_expires,
                tokens.mis_id,
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Option<Account>> {
        let connection = self.connection.lock();
        let raw = connection
            .query_row(
                &format!("SELECT {SELECT_COLUMNS} FROM accounts WHERE id = ?1"),
                params![id],
                map_account,
            )
            .optional()?;
        drop(connection);
        raw.map(|raw| self.decrypt_account(raw)).transpose()
    }

    pub fn list(&self) -> Result<Vec<Account>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM accounts WHERE status = 'active' ORDER BY id"
        ))?;
        let raw = statement
            .query_map([], map_account)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        drop(connection);
        raw.into_iter()
            .map(|account| self.decrypt_account(account))
            .collect()
    }

    /// Atomically pick and touch the least-recently-used active account.
    /// Accounts never used sort first, then oldest timestamp, then lowest id.
    pub fn select_lru(&self) -> Result<Option<Account>> {
        self.select_lru_at(chrono::Utc::now().timestamp_millis())
    }

    fn select_lru_at(&self, now_millis: i64) -> Result<Option<Account>> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw = transaction
            .query_row(
                &format!(
                    "SELECT {SELECT_COLUMNS} FROM accounts
                     WHERE status = 'active'
                     ORDER BY CASE WHEN last_used_at IS NULL THEN 0 ELSE 1 END,
                              last_used_at ASC, id ASC
                     LIMIT 1"
                ),
                [],
                map_account,
            )
            .optional()?;
        if let Some(account) = &raw {
            transaction.execute(
                "UPDATE accounts
                 SET last_used_at = ?2, total_requests = total_requests + 1,
                     updated_at = datetime('now')
                 WHERE id = ?1",
                params![account.id, now_millis],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        raw.map(|mut account| {
            account.last_used_at = Some(now_millis);
            account.total_requests += 1;
            self.decrypt_account(account)
        })
        .transpose()
    }

    pub fn update_tokens(&self, id: i64, tokens: &TokenSet) -> Result<()> {
        let access = self
            .cipher
            .encrypt(ACCESS_AAD, tokens.access_token.as_bytes())?;
        let refresh = self
            .cipher
            .encrypt(REFRESH_AAD, tokens.refresh_token.as_bytes())?;
        self.connection.lock().execute(
            "UPDATE accounts SET access_token = ?2, refresh_token = ?3,
             access_expires = ?4, refresh_expires = ?5,
             mis_id = COALESCE(?6, mis_id), updated_at = datetime('now')
             WHERE id = ?1",
            params![
                id,
                access,
                refresh,
                tokens.expires,
                tokens.refresh_expires,
                tokens.mis_id,
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        Ok(self
            .connection
            .lock()
            .execute("DELETE FROM accounts WHERE id = ?1", params![id])?
            != 0)
    }

    fn decrypt_account(&self, raw: RawAccount) -> Result<Account> {
        let access_token = String::from_utf8(self.cipher.decrypt(ACCESS_AAD, &raw.access_token)?)?;
        let refresh_token =
            String::from_utf8(self.cipher.decrypt(REFRESH_AAD, &raw.refresh_token)?)?;
        Ok(Account {
            id: raw.id,
            label: raw.label,
            mobile: raw.mobile,
            tokens: TokenSet {
                access_token,
                refresh_token,
                expires: raw.access_expires,
                refresh_expires: raw.refresh_expires,
                mis_id: raw.mis_id,
                user_info: None,
            },
            status: raw.status,
            total_requests: raw.total_requests,
            last_used_at: raw.last_used_at,
        })
    }
}

fn map_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawAccount> {
    Ok(RawAccount {
        id: row.get(0)?,
        label: row.get(1)?,
        mobile: row.get(2)?,
        access_token: row.get(3)?,
        refresh_token: row.get(4)?,
        access_expires: row.get(5)?,
        refresh_expires: row.get(6)?,
        mis_id: row.get(7)?,
        status: row.get(8)?,
        total_requests: row.get(9)?,
        last_used_at: row.get(10)?,
    })
}

#[cfg(unix)]
fn prepare_parent(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    if !parent.exists() {
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(unix)]
fn path_entry_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(unix)]
fn create_or_validate_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => {
            file.sync_all()?;
            validate_private_file(path)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            validate_private_file(path)
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(unix)]
fn validate_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(Error::UnsafePermissions(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(unix)]
fn load_or_create_key(path: &Path) -> Result<[u8; KEY_LEN]> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut created = false;
    let mut file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => {
            created = true;
            file
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            validate_private_file(path)?;
            OpenOptions::new()
                .read(true)
                .custom_flags(nix::libc::O_NOFOLLOW)
                .open(path)?
        }
        Err(error) => return Err(error.into()),
    };

    if created {
        let mut key = [0_u8; KEY_LEN];
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| Error::Crypto("account key generation failed".into()))?;
        file.write_all(&key)?;
        file.sync_all()?;
        validate_private_file(path)?;
        return Ok(key);
    }

    let mut key = [0_u8; KEY_LEN];
    file.read_exact(&mut key)?;
    let mut trailing = [0_u8; 1];
    if file.read(&mut trailing)? != 0 {
        return Err(Error::Crypto(format!(
            "account key {} has the wrong length",
            path.display()
        )));
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(access: &str, refresh: &str) -> TokenSet {
        TokenSet {
            access_token: access.into(),
            refresh_token: refresh.into(),
            expires: 100,
            refresh_expires: 200,
            mis_id: Some("mis".into()),
            user_info: None,
        }
    }

    #[test]
    #[cfg(unix)]
    fn sqlite_contains_only_ciphertext_and_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("accounts.db");
        let key = directory.path().join("accounts.key");
        let access = "very-secret-access-token";
        let refresh = "very-secret-refresh-token";
        {
            let store = AccountStore::open(&db, &key).unwrap();
            let id = store
                .insert("primary", None, &tokens(access, refresh))
                .unwrap();
            let restored = store.get(id).unwrap().unwrap();
            assert_eq!(restored.tokens.access_token, access);
            assert_eq!(restored.tokens.refresh_token, refresh);
        }
        let bytes = fs::read(&db).unwrap();
        assert!(
            !bytes
                .windows(access.len())
                .any(|window| window == access.as_bytes())
        );
        assert!(
            !bytes
                .windows(refresh.len())
                .any(|window| window == refresh.as_bytes())
        );
        assert_eq!(fs::metadata(&db).unwrap().permissions().mode() & 0o077, 0);
        assert_eq!(fs::metadata(&key).unwrap().permissions().mode() & 0o077, 0);
    }

    #[test]
    #[cfg(unix)]
    fn lru_selection_is_oldest_first_and_touches_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let store = AccountStore::open(
            directory.path().join("accounts.db"),
            directory.path().join("accounts.key"),
        )
        .unwrap();
        let first = store.insert("first", None, &tokens("a1", "r1")).unwrap();
        let second = store.insert("second", None, &tokens("a2", "r2")).unwrap();
        assert_eq!(store.select_lru_at(100).unwrap().unwrap().id, first);
        assert_eq!(store.select_lru_at(200).unwrap().unwrap().id, second);
        assert_eq!(store.select_lru_at(300).unwrap().unwrap().id, first);
    }

    #[test]
    #[cfg(unix)]
    fn unsafe_existing_key_permissions_fail_closed() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("accounts.db");
        let key = directory.path().join("accounts.key");
        fs::write(&key, [0_u8; KEY_LEN]).unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            AccountStore::open(db, key),
            Err(Error::UnsafePermissions(_))
        ));
    }
}
