//! Shared gitignore matching utility.
//!
//! Single source of truth for checking whether a path is ignored by
//! `.gitignore` rules. Used by both the initial AGENTS.md discovery
//! (`xai-grok-agent::prompt::ignore`) and the runtime tracker
//! (`AgentsMdTracker`).

use ignore::gitignore::Gitignore;
use std::path::Path;

/// Check if a path is ignored by the given gitignore rules.
///
/// Strips `git_root` prefix before matching — gitignore patterns are
/// repo-relative, so `/repo/build/out.o` becomes `build/out.o` when
/// `git_root` is `/repo`.
///
/// This is a pure function — no filesystem access, just `Gitignore::matched()`.
pub fn is_ignored(gitignore: &Gitignore, path: &Path, git_root: Option<&Path>) -> bool {
    let check_path = match git_root {
        Some(root) => match path.strip_prefix(root) {
            Ok(relative) => relative,
            // Outside the repo (e.g. ~/.grok/Agents.md) → not ignored.
            Err(_) => return false,
        },
        None => {
            // Absolute path + no git root → can't strip to repo-relative;
            // the `ignore` crate panics on absolute paths not under root.
            // `is_absolute` alone is not enough on Windows, where a leading
            // `/` is drive-relative rather than absolute; `has_root` is the
            // condition `ignore::gitignore` actually asserts against.
            if path.is_absolute() || path.has_root() {
                return false;
            }
            path
        }
    };
    // matched_path_or_any_parents checks parent dirs too, so
    // `build/AGENTS.md` correctly matches a `build/` pattern.
    gitignore
        .matched_path_or_any_parents(check_path, check_path.is_dir())
        .is_ignore()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignore::gitignore::GitignoreBuilder;

    fn build_gitignore(root: &Path, patterns: &[&str]) -> Gitignore {
        let mut builder = GitignoreBuilder::new(root);
        for pattern in patterns {
            builder.add_line(None, pattern).unwrap();
        }
        builder.build().unwrap()
    }

    #[test]
    fn is_ignored_matches_gitignored_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Canonicalize to handle macOS /tmp → /private/tmp
        let root = &dunce::canonicalize(root).unwrap();
        let gi = build_gitignore(root, &["build/"]);
        assert!(is_ignored(&gi, &root.join("build/out.o"), Some(root)));
        assert!(is_ignored(&gi, &root.join("build/sub/file.rs"), Some(root)));
    }

    #[test]
    fn is_ignored_does_not_match_non_gitignored_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let root = &dunce::canonicalize(root).unwrap();
        let gi = build_gitignore(root, &["build/"]);
        assert!(!is_ignored(&gi, &root.join("src/main.rs"), Some(root)));
        assert!(!is_ignored(&gi, &root.join("AGENTS.md"), Some(root)));
    }

    #[test]
    fn is_ignored_strips_git_root_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let root = &dunce::canonicalize(root).unwrap();
        let gi = build_gitignore(root, &["build/"]);
        // With root: strips prefix, matches build/out.o
        assert!(is_ignored(&gi, &root.join("build/out.o"), Some(root)));
        // Without root: relative path still matches
        assert!(is_ignored(
            &gi,
            &std::path::PathBuf::from("build/out.o"),
            None
        ));
    }

    #[test]
    fn is_ignored_returns_false_for_path_outside_git_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let root = &dunce::canonicalize(root).unwrap();
        let gi = build_gitignore(root, &["build/", "*.md"]);
        // A path completely outside the git root should not be checked
        // against the repo's .gitignore (e.g., ~/.grok/Agents.md).
        let outside_path = std::path::PathBuf::from("/some/other/path/Agents.md");
        assert!(!is_ignored(&gi, &outside_path, Some(root)));
    }

    /// Regression: running outside a git repo panicked with
    /// "path is expected to be under the root" (ignore crate assert).
    #[test]
    fn regression_no_panic_on_absolute_path_without_git_root() {
        let gi = build_gitignore(Path::new("."), &["node_modules/", "*.log"]);
        let abs_path = Path::new("/Users/someone/home/AGENTS.md");

        // Proves the raw crate panics with these inputs. Unix only: the crate's
        // precondition is `!path.has_root()`, and the windows-latest leg showed
        // the call returning normally there, so the panic demonstration is a
        // unix path-parsing fact rather than part of this crate's contract.
        #[cfg(unix)]
        assert!(
            std::panic::catch_unwind(|| {
                gi.matched_path_or_any_parents(abs_path, false);
            })
            .is_err()
        );

        // Our wrapper guards against it on every platform.
        assert!(!is_ignored(&gi, abs_path, None));
    }

    /// The guard has to cover exactly what `ignore::gitignore` asserts against,
    /// which is `has_root()` rather than `is_absolute()`: on Windows a leading
    /// `/` is drive-relative and not absolute, so a root-shaped path could
    /// otherwise reach a matcher that only accepts repo-relative paths.
    #[test]
    fn a_root_shaped_path_never_reaches_the_matcher() {
        let gi = build_gitignore(Path::new("."), &["build/"]);
        for rooted in ["/Users/someone/home/build/out.o", "/build/out.o"] {
            let path = Path::new(rooted);
            assert!(path.has_root(), "{rooted} is not root-shaped");
            assert!(
                !is_ignored(&gi, path, None),
                "{rooted} is outside any repo, so no repo rule can ignore it"
            );
        }
    }
}
