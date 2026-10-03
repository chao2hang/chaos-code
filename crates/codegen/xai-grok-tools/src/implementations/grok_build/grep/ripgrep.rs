use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use xai_tool_runtime::{ToolError, ToolErrorKind};

#[cfg(bundle_rg)]
const RG_BYTES: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/bundle-rg/rg-",
    env!("GROK_TOOLS_RG_VER"),
    "-",
    env!("GROK_TOOLS_RG_TARGET"),
    ".bin.zst"
));

#[cfg(bundle_rg)]
fn resolve_bundled_rg() -> Result<Option<PathBuf>, crate::util::vendor::InstallError> {
    crate::util::vendor::resolve(
        concat!(
            "rg-",
            env!("GROK_TOOLS_RG_VER"),
            "-",
            env!("GROK_TOOLS_RG_TARGET")
        ),
        RG_BYTES,
        env!("GROK_TOOLS_RG_SHA256"),
    )
}

/// File name of the ripgrep binary for the build target.
fn rg_file_name() -> &'static str {
    if cfg!(windows) { "rg.exe" } else { "rg" }
}

/// Install directories checked after `PATH`.
///
/// A process started by a desktop session inherits the session's `PATH`, not the
/// interactive shell's, so a Homebrew, apt or cargo install that works in a
/// terminal is invisible to a bare `rg` lookup. Debug builds do not embed
/// ripgrep (only release builds bundle it), so this is the only fallback those
/// builds have.
///
/// The list is per-OS because the package managers differ: the unix entries are
/// not even absolute on Windows (`/opt/homebrew/bin` is drive-relative there),
/// and a Windows user installs ripgrep through winget, Chocolatey or Scoop.
fn rg_install_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = if cfg!(windows) {
        vec![
            // Chocolatey shims; the one install dir that is machine-wide.
            PathBuf::from(r"C:\ProgramData\chocolatey\bin"),
        ]
    } else {
        vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]
    };
    if let Some(home) = home {
        dirs.push(home.join(".cargo").join("bin"));
        if cfg!(windows) {
            dirs.push(home.join("scoop").join("shims"));
            dirs.push(
                home.join("AppData")
                    .join("Local")
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links"),
            );
        }
    }
    dirs
}

/// Bazel runfile holding a hermetic ripgrep, if this binary was built with one.
fn rg_runfile(runfiles_dir: &Path, exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    for entry in std::fs::read_dir(runfiles_dir).ok()?.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .contains("ripgrep_hermetic")
        {
            continue;
        }
        for sub in ["amd64/rg", "arm64/rg", "rg"] {
            let candidate = entry.path().join(sub);
            if exists(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Resolve the ripgrep binary the search tools will spawn.
///
/// Order: the `RG_BIN_PATH` override, a Bazel runfile, `PATH`, then the usual
/// install directories. A binary that cannot be found is reported here rather
/// than at spawn time, where it surfaced as a bare `No such file or directory`
/// that named neither ripgrep nor the way to fix it.
///
/// `exists` is injected so the lookup order is testable without touching the
/// host's filesystem.
fn resolve_rg(
    explicit: Option<&OsStr>,
    runfiles_dir: Option<&OsStr>,
    path_var: Option<&OsStr>,
    home: Option<&Path>,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<PathBuf, String> {
    if let Some(explicit) = explicit {
        let path = PathBuf::from(explicit);
        return if exists(&path) {
            Ok(path)
        } else {
            Err(format!(
                "RG_BIN_PATH points at {}, which is not a file",
                path.display()
            ))
        };
    }

    if let Some(runfiles_dir) = runfiles_dir
        && let Some(found) = rg_runfile(Path::new(runfiles_dir), exists)
    {
        return Ok(found);
    }

    if let Some(path_var) = path_var {
        for dir in std::env::split_paths(path_var).filter(|dir| !dir.as_os_str().is_empty()) {
            let candidate = dir.join(rg_file_name());
            if exists(&candidate) {
                return Ok(candidate);
            }
        }
    }

    if let Some(found) = rg_install_dirs(home)
        .into_iter()
        .map(|dir| dir.join(rg_file_name()))
        .find(|candidate| exists(candidate))
    {
        return Ok(found);
    }

    Err(
        "ripgrep was not found: install ripgrep, or set RG_BIN_PATH to its absolute path \
         (release builds embed it, debug builds use the host's)"
            .to_string(),
    )
}

pub fn rg_path() -> Result<PathBuf, ToolError> {
    static RG_EXEC: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    RG_EXEC
        .get_or_init(|| {
            #[cfg(bundle_rg)]
            {
                match resolve_bundled_rg() {
                    Ok(Some(path)) => Ok(path),
                    // A release build whose vendored copy could not be unpacked
                    // still has a working host ripgrep.
                    Ok(None) | Err(_) => resolve_rg(
                        std::env::var_os("RG_BIN_PATH").as_deref(),
                        std::env::var_os("RUNFILES_DIR").as_deref(),
                        std::env::var_os("PATH").as_deref(),
                        xai_dirs::home_dir().as_deref(),
                        &Path::is_file,
                    ),
                }
            }
            #[cfg(not(bundle_rg))]
            {
                resolve_rg(
                    std::env::var_os("RG_BIN_PATH").as_deref(),
                    std::env::var_os("RUNFILES_DIR").as_deref(),
                    std::env::var_os("PATH").as_deref(),
                    xai_dirs::home_dir().as_deref(),
                    &Path::is_file,
                )
            }
        })
        .clone()
        .map_err(|msg| ToolError::new(ToolErrorKind::Execution, msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Filesystem stand-in: the candidates a build would accept as ripgrep.
    fn existing(accepted: &[&str]) -> impl Fn(&Path) -> bool {
        let accepted: Vec<PathBuf> = accepted.iter().map(PathBuf::from).collect();
        move |path: &Path| accepted.iter().any(|candidate| candidate == path)
    }

    #[test]
    fn an_explicit_override_is_used_verbatim() {
        let resolved = resolve_rg(
            Some(OsStr::new("/opt/pin/rg")),
            None,
            Some(OsStr::new("/usr/bin")),
            None,
            &existing(&["/opt/pin/rg"]),
        )
        .unwrap();
        assert_eq!(resolved, PathBuf::from("/opt/pin/rg"));
    }

    #[test]
    fn a_override_pointing_at_nothing_is_reported_rather_than_ignored() {
        // Falling through to PATH here would leave a broken deployment running a
        // different ripgrep than its operator pinned, silently.
        let error = resolve_rg(
            Some(OsStr::new("/opt/pin/rg")),
            None,
            Some(OsStr::new("/usr/bin")),
            None,
            &existing(&["/usr/bin/rg"]),
        )
        .expect_err("a missing override must not pass silently");
        assert!(error.contains("RG_BIN_PATH"), "{error}");
        assert!(error.contains("/opt/pin/rg"), "{error}");
    }

    #[test]
    fn path_is_searched_left_to_right() {
        let joined = format!("/first{}second", if cfg!(windows) { ";" } else { ":" });
        let resolved = resolve_rg(
            None,
            None,
            Some(OsStr::new(&joined)),
            None,
            &existing(&[
                PathBuf::from("/first")
                    .join(rg_file_name())
                    .to_str()
                    .unwrap(),
                PathBuf::from("/second")
                    .join(rg_file_name())
                    .to_str()
                    .unwrap(),
            ]),
        )
        .unwrap();
        assert_eq!(resolved, PathBuf::from("/first").join(rg_file_name()));
    }

    #[test]
    fn an_empty_path_entry_is_not_searched() {
        // `PATH=""` and `PATH=":"` must not resolve to a relative `rg`.
        let error = resolve_rg(None, None, Some(OsStr::new(":")), None, &existing(&["rg"]))
            .expect_err("a relative candidate is not a resolution");
        assert!(error.contains("ripgrep was not found"), "{error}");
    }

    #[test]
    #[cfg(unix)]
    fn a_gui_session_path_still_finds_a_standard_install() {
        // The macOS case: launched from the desktop, `PATH` has no homebrew and
        // no cargo bin, yet ripgrep is installed at /opt/homebrew/bin. The dir
        // only exists on unix, so this scenario has no Windows counterpart.
        let resolved = resolve_rg(
            None,
            None,
            Some(OsStr::new("/usr/bin:/bin")),
            None,
            &existing(&[
                "/usr/bin/bash",
                PathBuf::from("/opt/homebrew/bin")
                    .join(rg_file_name())
                    .to_str()
                    .unwrap(),
            ]),
        )
        .unwrap();
        assert_eq!(
            resolved,
            PathBuf::from("/opt/homebrew/bin").join(rg_file_name())
        );
    }

    #[test]
    fn a_cargo_install_under_home_is_found_last() {
        let resolved = resolve_rg(
            None,
            None,
            Some(OsStr::new("/usr/bin")),
            Some(Path::new("/Users/dev")),
            &existing(&[PathBuf::from("/Users/dev/.cargo/bin")
                .join(rg_file_name())
                .to_str()
                .unwrap()]),
        )
        .unwrap();
        assert_eq!(
            resolved,
            PathBuf::from("/Users/dev/.cargo/bin").join(rg_file_name())
        );
    }

    #[test]
    fn candidates_use_the_target_specific_binary_name() {
        let resolved = resolve_rg(
            None,
            None,
            Some(OsStr::new("/usr/bin")),
            None,
            &existing(&[PathBuf::from("/usr/bin")
                .join(rg_file_name())
                .to_str()
                .unwrap()]),
        )
        .unwrap();
        assert_eq!(resolved.file_name(), Some(OsStr::new(rg_file_name())));
    }

    #[test]
    fn a_missing_binary_names_the_override_that_would_fix_it() {
        let error = resolve_rg(
            None,
            None,
            Some(OsStr::new("/usr/bin")),
            None,
            &existing(&[]),
        )
        .expect_err("no ripgrep is an error, not a bare `rg`");
        assert!(error.contains("ripgrep was not found"), "{error}");
        assert!(error.contains("RG_BIN_PATH"), "{error}");
    }

    #[test]
    fn a_bazel_runfile_wins_over_path() {
        let root = std::env::temp_dir().join(format!(
            "grok-rg-runfiles-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let hermetic = root.join("ripgrep_hermetic_arm64");
        std::fs::create_dir_all(hermetic.join("arm64")).unwrap();
        let runfile = hermetic.join("arm64/rg");
        std::fs::write(&runfile, b"#!/bin/sh\n").unwrap();

        let resolved = resolve_rg(
            None,
            Some(root.as_os_str()),
            Some(OsStr::new("/usr/bin")),
            None,
            &|path: &Path| path.exists(),
        );

        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(resolved.unwrap(), runfile);
    }

    #[test]
    fn a_runfile_without_a_binary_falls_through_to_path() {
        let root = std::env::temp_dir().join(format!(
            "grok-rg-runfiles-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("ripgrep_hermetic_amd64")).unwrap();

        let resolved = resolve_rg(
            None,
            Some(root.as_os_str()),
            Some(OsStr::new("/usr/bin")),
            None,
            &existing(&[PathBuf::from("/usr/bin")
                .join(rg_file_name())
                .to_str()
                .unwrap()]),
        );

        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            resolved.unwrap(),
            PathBuf::from("/usr/bin").join(rg_file_name())
        );
    }

    #[test]
    fn install_dirs_are_absolute_and_cover_the_known_package_managers() {
        let home = if cfg!(windows) {
            Path::new(r"C:\Users\dev")
        } else {
            Path::new("/home/dev")
        };
        let dirs = rg_install_dirs(Some(home));
        assert!(
            dirs.iter().all(|dir| dir.is_absolute()),
            "a relative install dir would resolve against the process cwd: {dirs:?}"
        );
        // Cargo is the one manager whose layout is the same on both platforms.
        assert!(dirs.contains(&home.join(".cargo").join("bin")), "{dirs:?}");
        if cfg!(windows) {
            assert!(
                dirs.contains(&PathBuf::from(r"C:\ProgramData\chocolatey\bin")),
                "{dirs:?}"
            );
            assert!(dirs.contains(&home.join("scoop").join("shims")), "{dirs:?}");
            assert!(
                dirs.contains(
                    &home
                        .join("AppData")
                        .join("Local")
                        .join("Microsoft")
                        .join("WinGet")
                        .join("Links")
                ),
                "{dirs:?}"
            );
        } else {
            assert!(
                dirs.contains(&PathBuf::from("/opt/homebrew/bin")),
                "{dirs:?}"
            );
            assert!(dirs.contains(&PathBuf::from("/usr/local/bin")), "{dirs:?}");
        }
    }

    #[test]
    fn install_dirs_are_absolute_when_there_is_no_home_directory() {
        // The entries that do not depend on `home` are the ones a Windows build
        // used to get wrong: `/opt/homebrew/bin` is drive-relative there, so a
        // relative candidate would be resolved against the process cwd.
        let dirs = rg_install_dirs(None);
        assert!(!dirs.is_empty());
        assert!(dirs.iter().all(|dir| dir.is_absolute()), "{dirs:?}");
    }
}
