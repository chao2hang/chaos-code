//! A path *inside* a remote workspace, kept distinct from a local one.
//!
//! The remote topology sends paths over a wire and then opens them. The failure
//! mode worth designing against is not a missing file — it is a path meaning
//! something other than what the caller meant: a `..` that leaves the workspace,
//! a symlink that does the same thing more quietly, or a local absolute path
//! pasted into a request that then gets opened on whichever machine happens to
//! be holding the file handle.
//!
//! So a remote path is its own type. It is only ever built by
//! [`RemotePath::parse`], which resolves it against a root and rejects anything
//! that would escape; the [`std::fmt::Display`] form is prefixed `remote:` so a
//! path that escapes into a log, a UI string or a `file://` deep link announces
//! itself rather than being opened locally.

use std::fmt;
use std::path::{Component, Path, PathBuf};

/// A path resolved to live inside one remote workspace root.
///
/// The inner value is the workspace-relative form, with `/` separators on every
/// platform, which is what makes it meaningful to a peer that may be running a
/// different OS than the one that produced it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RemotePath(String);

impl RemotePath {
    /// The workspace-relative path, `/`-separated. Never absolute, never empty.
    pub fn relative(&self) -> &str {
        &self.0
    }

    /// Resolve `relative` inside `root`, refusing to leave it.
    ///
    /// Rejection is deliberate at every step, because each step is a different
    /// way to escape:
    /// - an absolute path, or one with a Windows drive or prefix, names a
    ///   location on the *server* rather than in the workspace;
    /// - `..` is refused textually rather than resolved and compared, so a
    ///   request cannot probe how far outside the root the server would have
    ///   been willing to go;
    /// - a symlink is resolved on disk and the *resolved* target has to be under
    ///   the resolved root, because `root/sub/link -> /etc` passes any textual
    ///   check.
    pub fn parse(root: &Path, relative: &str) -> Result<Self, PathRejection> {
        let trimmed = relative.trim();
        if trimmed.is_empty() {
            return Err(PathRejection::Empty);
        }
        let candidate = Path::new(trimmed);
        if candidate.is_absolute() {
            return Err(PathRejection::Absolute(trimmed.to_string()));
        }
        let mut parts: Vec<String> = Vec::new();
        for comp in candidate.components() {
            match comp {
                Component::Normal(part) => {
                    let part = part.to_string_lossy();
                    if part == "." {
                        continue;
                    }
                    parts.push(part.into_owned());
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err(PathRejection::Escapes(trimmed.to_string()));
                }
                // A drive or UNC prefix (`C:`, `\\server\share`) names a place on
                // the server, and `Path::is_absolute` does not report it as
                // absolute on Windows.
                Component::RootDir | Component::Prefix(_) => {
                    return Err(PathRejection::Absolute(trimmed.to_string()));
                }
            }
        }
        if parts.is_empty() {
            return Err(PathRejection::Empty);
        }
        let joined = parts
            .iter()
            .fold(root.to_path_buf(), |acc, part| acc.join(part));
        // The root is canonicalised because a caller may hand us `/var/…` while
        // the OS reports `/private/var/…`; comparing an unresolved root to a
        // resolved target would reject every path on macOS.
        let root = canonical(root);
        let resolved = resolve_existing(&joined);
        if !starts_with(&resolved, &root) {
            return Err(PathRejection::Escapes(trimmed.to_string()));
        }
        Ok(Self(parts.join("/")))
    }

    /// The absolute local path this corresponds to under `root`.
    ///
    /// Deliberately an explicit call with the root in hand: the only way to get
    /// a `PathBuf` out of a `RemotePath` is to say which workspace it belongs
    /// to, so a remote path cannot quietly be handed to a local file API.
    pub fn to_local(&self, root: &Path) -> PathBuf {
        self.0
            .split('/')
            .fold(root.to_path_buf(), |acc, part| acc.join(part))
    }
}

impl fmt::Display for RemotePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "remote:{}", self.0)
    }
}

/// Why a path was not accepted as a path inside the workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathRejection {
    /// Nothing to point at.
    Empty,
    /// The request named a location on the server's own filesystem.
    Absolute(String),
    /// The request leaves the workspace, textually or through a symlink.
    Escapes(String),
}

impl fmt::Display for PathRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "path is empty"),
            Self::Absolute(p) => write!(
                f,
                "absolute paths name the server's filesystem, not the workspace: {p}"
            ),
            Self::Escapes(p) => write!(f, "path leaves the workspace: {p}"),
        }
    }
}

impl std::error::Error for PathRejection {}

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Canonicalise the deepest existing ancestor and put the rest back.
///
/// The final component usually does not exist yet — that is the point of a
/// write — and `canonicalize` needs the file to be there.
fn resolve_existing(path: &Path) -> PathBuf {
    let mut missing: Vec<&std::ffi::OsStr> = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        match cursor.file_name() {
            Some(name) => missing.push(name),
            None => break,
        }
        match cursor.parent() {
            Some(parent) => cursor = parent,
            None => break,
        }
    }
    let mut resolved = canonical(cursor);
    for name in missing.iter().rev() {
        resolved.push(name);
    }
    resolved
}

/// `base` is `path` or one of its ancestors, component by component.
///
/// A `starts_with` on the string form would accept `/work` as a prefix of
/// `/workspace`, which is a different directory.
fn starts_with(path: &Path, base: &Path) -> bool {
    path == base || path.starts_with(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        dir
    }

    #[test]
    fn a_plain_relative_path_is_accepted() {
        let root = root();
        let path = RemotePath::parse(root.path(), "src/main.rs").unwrap();
        assert_eq!(path.relative(), "src/main.rs");
        assert_eq!(
            path.to_local(root.path()),
            root.path().join("src").join("main.rs")
        );
        assert_eq!(path.to_string(), "remote:src/main.rs");
    }

    #[test]
    fn nested_separators_are_normalised() {
        let root = root();
        std::fs::create_dir_all(root.path().join("a/b")).unwrap();
        let path = RemotePath::parse(root.path(), "./a//b/./c.rs").unwrap();
        assert_eq!(path.relative(), "a/b/c.rs");
    }

    #[test]
    fn an_absolute_path_is_refused() {
        let root = root();
        assert_eq!(
            RemotePath::parse(root.path(), "/etc/passwd"),
            Err(PathRejection::Absolute("/etc/passwd".into()))
        );
    }

    #[test]
    fn a_parent_traversal_is_refused() {
        let root = root();
        for attempt in ["../secret", "src/../../secret", "src/../src/main.rs"] {
            assert!(
                RemotePath::parse(root.path(), attempt).is_err(),
                "{attempt} must not be resolvable; a traversal that happens to land \
                 back inside still lets the caller probe the tree"
            );
        }
    }

    #[test]
    fn an_empty_path_is_refused() {
        let root = root();
        assert_eq!(
            RemotePath::parse(root.path(), "   "),
            Err(PathRejection::Empty)
        );
        assert_eq!(
            RemotePath::parse(root.path(), "."),
            Err(PathRejection::Empty)
        );
    }

    /// The textual checks are not enough on their own: a link written by an
    /// earlier write is the way out of a workspace that looks well-behaved.
    #[test]
    #[cfg(unix)]
    fn a_symlink_pointing_out_of_the_workspace_is_refused() {
        let root = root();
        std::os::unix::fs::symlink("/etc", root.path().join("src/etc")).unwrap();
        assert_eq!(
            RemotePath::parse(root.path(), "src/etc/passwd"),
            Err(PathRejection::Escapes("src/etc/passwd".into()))
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_symlink_staying_inside_the_workspace_is_accepted() {
        let root = root();
        std::os::unix::fs::symlink("main.rs", root.path().join("src/copy.rs")).unwrap();
        assert!(RemotePath::parse(root.path(), "src/copy.rs").is_ok());
    }

    /// A path that does not exist yet still resolves inside the root — that is
    /// the ordinary case for a write.
    #[test]
    fn a_path_that_does_not_exist_yet_is_judged_on_where_it_would_land() {
        let root = root();
        assert!(RemotePath::parse(root.path(), "src/new.rs").is_ok());
        assert!(RemotePath::parse(root.path(), "deeply/nested/new.rs").is_ok());
    }

    #[test]
    fn a_sibling_directory_is_not_mistaken_for_the_root() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("work");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(outer.path().join("sibling.txt"), "x").unwrap();
        // `work/../work-sibling/x` is refused textually, and `..` never gets to
        // be resolved against a real filesystem here.
        assert!(RemotePath::parse(&root, "../work-sibling/x").is_err());
    }
}
