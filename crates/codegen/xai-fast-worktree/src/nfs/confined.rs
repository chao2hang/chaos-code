//! Fd-relative helpers + the single owned deleter used by daemon-down `rm`
//! and `clean-artifacts`. Never a weaker sibling of `grove_git::delete_owned`.
pub fn is_safe_worktree_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && !id.contains('/')
        && !id.contains('\\')
        // Whitespace and control characters (NUL, newline, tab, DEL) break both
        // consumers: the pin ref `refs/grok/worktrees/<id>`, which git refuses to
        // create with a space or a newline in it, and the backing-marker dirent
        // lookup, which reads the id back from a file name. Ids built by
        // `worktree::plan::worktree_id_from_path` sanitize to `[A-Za-z0-9._-]`,
        // so this only rejects ids that arrive from outside.
        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}
