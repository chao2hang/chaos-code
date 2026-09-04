//! Home-directory resolution generally: USERPROFILE-first `home_dir`, plus
//! grok/chaos-home (`$CHAOS_HOME`, `$GROK_HOME`, `<home>/.chaos` or the
//! legacy `<home>/.grok`). Shared by `xai-grok-config` and
//! `xai-fast-worktree`.
//!
//! Dual-home policy (never copies or overwrites either tree):
//! 1. Non-empty `$CHAOS_HOME` (priority).
//! 2. Non-empty `$GROK_HOME` (legacy harnesses and docs still inject it).
//! 3. Existing `<home>/.chaos`.
//! 4. Existing `<home>/.grok` (legacy install).
//! 5. Default `<home>/.chaos` for new installs.
//!
//! Which function to call:
//! - [`grok_home`]: the usual choice, a cached, created path to build on.
//! - [`user_grok_home`]: `None` instead of a cwd fallback when no home resolves.
//! - [`default_grok_home`]: the no-env default (with the dual-dir preference), so callers can detect an override.
//! - [`resolve_grok_home`]: a fresh, uncached resolve.
//! - [`resolve_grok_home_with_source`]: [`resolve_grok_home`] plus where the path came from.
//! - [`home_dir`]: the home directory itself, for sibling dot dirs (`~/.claude`, `~/.agents`, ...).
//!
//! TODO: collapse these getters by threading the path through config as an
//! explicit value.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Chaos-native user home directory name (`~/.chaos`).
pub const CHAOS_HOME_DIRNAME: &str = ".chaos";
/// Legacy Grok Build user home directory name (`~/.grok`), still dual-read.
pub const LEGACY_GROK_HOME_DIRNAME: &str = ".grok";

/// Where a resolved grok home came from, so "why did chaos pick this
/// directory?" is answerable in diagnostics without re-reading the
/// environment at the asking site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrokHomeSource {
    /// A non-empty `$CHAOS_HOME` or `$GROK_HOME` override.
    EnvOverride,
    /// `<home>/.chaos` derived from the home directory (existing tree or
    /// new-install default).
    ChaosDefault,
    /// An existing legacy `<home>/.grok` tree.
    LegacyGrok,
}

/// The user's home directory via [`std::env::home_dir`]: `HOME` on Unix (with
/// a passwd fallback), `USERPROFILE` on Windows.
///
/// Deliberately not `dirs::home_dir()`: on Windows `dirs` asks the
/// known-folder API and ignores a redirected `USERPROFILE`, while this crate
/// resolves `~/.grok` from the profile variable — mixing the two sources puts
/// the grok directory and other home-anchored dot directories in different
/// trees. Every home-anchored path must come from this one function.
#[allow(deprecated, clippy::disallowed_methods)] // the one sanctioned std::env::home_dir call
pub fn home_dir() -> Option<PathBuf> {
    std::env::home_dir()
}

/// Canonicalize a home via `dunce` (not `std::fs::canonicalize`, which yields
/// Windows `\\?\` verbatim paths).
fn canonical_home(home: &Path) -> PathBuf {
    dunce::canonicalize(home).unwrap_or_else(|_| home.to_path_buf())
}

/// Dual-dir preference under a canonicalized user home: prefer an existing
/// `.chaos`, then an existing legacy `.grok`, else default to `.chaos`.
fn dual_default_home_in(home: &Path) -> (PathBuf, GrokHomeSource) {
    let base = canonical_home(home);
    let chaos = base.join(CHAOS_HOME_DIRNAME);
    let grok = base.join(LEGACY_GROK_HOME_DIRNAME);
    if chaos.is_dir() {
        (chaos, GrokHomeSource::ChaosDefault)
    } else if grok.is_dir() {
        (grok, GrokHomeSource::LegacyGrok)
    } else {
        (chaos, GrokHomeSource::ChaosDefault)
    }
}

/// `$CHAOS_HOME` then `$GROK_HOME` verbatim when non-empty, else the
/// dual-dir default under `os_home`. Env values are used as-is (not
/// canonicalized) so they stay stable and comparable: callers do literal
/// prefix checks against them, and downstream symlink guards must still see
/// their original components.
fn resolve_grok_home_from(
    chaos_home_env: Option<&OsStr>,
    grok_home_env: Option<&OsStr>,
    os_home: Option<&Path>,
) -> Option<(PathBuf, GrokHomeSource)> {
    if let Some(env) = chaos_home_env.filter(|env| !env.is_empty()) {
        return Some((PathBuf::from(env), GrokHomeSource::EnvOverride));
    }
    if let Some(env) = grok_home_env.filter(|env| !env.is_empty()) {
        return Some((PathBuf::from(env), GrokHomeSource::EnvOverride));
    }
    os_home.map(|home| dual_default_home_in(home))
}

/// Resolve the chaos/grok home from the environment (fresh, no cache); `None` if nothing resolves.
pub fn resolve_grok_home() -> Option<PathBuf> {
    resolve_grok_home_with_source().map(|(home, _)| home)
}

/// [`resolve_grok_home`] plus the [`GrokHomeSource`] the path came from.
pub fn resolve_grok_home_with_source() -> Option<(PathBuf, GrokHomeSource)> {
    resolve_grok_home_from(
        std::env::var_os("CHAOS_HOME").as_deref(),
        std::env::var_os("GROK_HOME").as_deref(),
        home_dir().as_deref(),
    )
}

/// The no-env default home: dual-dir preference under `<home>` (existing
/// `~/.chaos`, else existing legacy `~/.grok`, else `~/.chaos`).
pub fn default_grok_home() -> PathBuf {
    dual_default_home_in(&home_dir().unwrap_or_else(|| PathBuf::from("."))).0
}

/// The chaos/grok home, created if missing and cached for the process; falls
/// back to [`default_grok_home`] when neither env var nor a home resolves.
pub fn grok_home() -> PathBuf {
    static GROK_HOME: OnceLock<PathBuf> = OnceLock::new();
    GROK_HOME
        .get_or_init(|| {
            let home = resolve_grok_home().unwrap_or_else(default_grok_home);
            if let Err(err) = std::fs::create_dir_all(&home) {
                tracing::warn!(path = %home.display(), %err, "failed to create chaos home");
            }
            home
        })
        .clone()
}

/// Like [`grok_home`], but `None` when no home resolves (no cwd fallback).
pub fn user_grok_home() -> Option<PathBuf> {
    resolve_grok_home().is_some().then(grok_home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::ffi::OsString;

    #[test]
    fn chaos_env_wins_over_grok_env_and_os_home() {
        let resolved = resolve_grok_home_from(
            Some(OsStr::new("/chaos/home")),
            Some(OsStr::new("/grok/home")),
            Some(Path::new("/home/u")),
        );
        assert_eq!(
            resolved,
            Some((PathBuf::from("/chaos/home"), GrokHomeSource::EnvOverride))
        );
    }

    #[test]
    fn grok_env_wins_over_os_home() {
        let resolved = resolve_grok_home_from(
            None,
            Some(OsStr::new("/custom/home")),
            Some(Path::new("/home/u")),
        );
        assert_eq!(
            resolved,
            Some((PathBuf::from("/custom/home"), GrokHomeSource::EnvOverride))
        );
    }

    #[test]
    fn env_used_verbatim_even_when_it_exists() {
        // A real, existing dir whose canonical form differs (macOS symlinks
        // `/var` -> `/private/var`): the env value must come back unchanged.
        let tmp = tempfile::tempdir().unwrap();
        let resolved = resolve_grok_home_from(None, Some(tmp.path().as_os_str()), None);
        assert_eq!(
            resolved,
            Some((tmp.path().to_path_buf(), GrokHomeSource::EnvOverride))
        );
    }

    #[test]
    fn empty_envs_fall_through_to_dual_default() {
        // Neither <tmp>/.chaos nor <tmp>/.grok exists yet → new-install
        // default <tmp>/.chaos.
        let tmp = tempfile::tempdir().unwrap();
        let resolved =
            resolve_grok_home_from(Some(&OsString::new()), Some(&OsString::new()), Some(tmp.path()));
        assert_eq!(
            resolved,
            Some((
                dual_default_home_in(tmp.path()).0,
                GrokHomeSource::ChaosDefault
            ))
        );
    }

    #[test]
    fn existing_legacy_grok_dir_is_preferred_when_chaos_absent() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(LEGACY_GROK_HOME_DIRNAME)).unwrap();
        let resolved = resolve_grok_home_from(None, None, Some(tmp.path()));
        assert_eq!(
            resolved,
            Some((
                dunce::canonicalize(tmp.path()).unwrap().join(".grok"),
                GrokHomeSource::LegacyGrok
            ))
        );
    }

    #[test]
    fn existing_chaos_dir_wins_over_legacy_grok() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(CHAOS_HOME_DIRNAME)).unwrap();
        std::fs::create_dir(tmp.path().join(LEGACY_GROK_HOME_DIRNAME)).unwrap();
        let resolved = resolve_grok_home_from(None, None, Some(tmp.path()));
        assert_eq!(
            resolved,
            Some((
                dunce::canonicalize(tmp.path()).unwrap().join(".chaos"),
                GrokHomeSource::ChaosDefault
            ))
        );
    }

    #[test]
    fn default_grok_home_has_no_verbatim_prefix() {
        // The reason we canonicalize via dunce: std::fs::canonicalize yields
        // `\\?\` verbatim paths on Windows that break git and byte-exact
        // comparisons. No-op assertion on Unix.
        let home = default_grok_home();
        assert!(!home.to_string_lossy().starts_with(r"\\?\"));
        assert!(home.ends_with(CHAOS_HOME_DIRNAME) || home.ends_with(LEGACY_GROK_HOME_DIRNAME));
    }

    #[test]
    fn none_when_nothing_resolves() {
        assert_eq!(
            resolve_grok_home_from(/* chaos_home_env */ None, /* grok_home_env */ None, /* os_home */ None),
            None
        );
    }
}
