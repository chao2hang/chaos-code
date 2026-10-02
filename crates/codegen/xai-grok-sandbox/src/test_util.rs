//! Test-only fixtures shared by the profile and runtime-socket policy tests.

use crate::profiles::{ProfileConfig, ProfileName, SandboxConfig};
use std::collections::HashMap;

/// A freshly created temp directory that a Unix socket name can still fit in.
///
/// `sockaddr_un::sun_path` is 104 bytes on macOS against Linux's 108, and a
/// fixture's endpoint is not the end of the path: the deepest one appended is
/// `.docker/desktop/docker.sock`. macOS also parks `TMPDIR` under
/// `/var/folders/…/T/`, ~50 bytes before a fixture adds anything of its own.
/// A root built from `temp_dir()` alone therefore overflows the kernel limit on
/// a Mac and the test dies in `UnixListener::bind` with `InvalidInput` instead
/// of ever reaching its assertion. `/tmp` is short and real on both platforms,
/// so that is what a too-long `TMPDIR` gives way to — honoured where it fits,
/// because a run that went to the trouble of setting it should be respected.
///
/// Canonical, because the policy under test canonicalises the endpoints it is
/// handed (`normalize_existing_parent_alias`) and a fixture handing it an
/// unresolved path would be testing a symlink hop rather than the policy. On
/// macOS `/var` — and therefore `TMPDIR` — *is* a symlink to `/private/var`.
#[cfg(unix)]
pub(crate) fn short_socket_root(prefix: &str, tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let leaf = format!("{prefix}-{tag}-{}-{nanos}", std::process::id());
    let tmpdir = dunce::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
    /// Longest endpoint a fixture appends below the root.
    const DEEPEST_ENDPOINT: usize = ".docker/desktop/docker.sock".len();
    let base = if tmpdir.as_os_str().len() + leaf.len() + 1 + DEEPEST_ENDPOINT <= 104 {
        tmpdir
    } else {
        dunce::canonicalize("/tmp").unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
    };
    let root = base.join(leaf);
    std::fs::create_dir_all(&root).unwrap();
    let root = dunce::canonicalize(&root).unwrap_or(root);
    assert!(
        probe_socket_name_fits(&root),
        "fixture root {} cannot hold a Unix socket name",
        root.display()
    );
    root
}

/// Whether a socket can actually be bound under `dir`, checked at the depth the
/// fixtures use rather than trusted to an arithmetic estimate.
#[cfg(unix)]
fn probe_socket_name_fits(dir: &std::path::Path) -> bool {
    use std::os::unix::net::UnixListener;

    let endpoint = dir.join("probe.sock");
    match UnixListener::bind(&endpoint) {
        Ok(_) => {
            let _ = std::fs::remove_file(&endpoint);
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => false,
        Err(e) => panic!("probe bind under {} failed: {e}", dir.display()),
    }
}

/// Hosts with a retargetable `$GROK_HOME/hooks` symlink (fail-closed under write-deny) cannot resolve enforcing profiles against the real home.
pub(crate) fn skip_if_host_hook_write_deny_unresolvable() -> bool {
    if !crate::hook_write_deny::profile_enforces_hook_write_deny(&ProfileName::Workspace) {
        return false;
    }
    match crate::hook_write_deny::resolve_hook_write_deny_snapshot() {
        Ok(_) => false,
        Err(e) => {
            eprintln!("skipping profile resolve test: host hook write-deny unresolvable ({e})");
            true
        }
    }
}

/// Custom profiles covering every restrict_network inheritance/override shape.
pub(crate) fn network_inheritance_config() -> SandboxConfig {
    SandboxConfig {
        profiles: HashMap::from([
            (
                "strict-inherited".to_string(),
                ProfileConfig {
                    extends: Some("strict".to_string()),
                    restrict_network: None,
                    read_only: vec![],
                    read_write: vec![],
                    deny: vec![],
                },
            ),
            (
                "read-only-inherited".to_string(),
                ProfileConfig {
                    extends: Some("read-only".to_string()),
                    restrict_network: None,
                    read_only: vec![],
                    read_write: vec![],
                    deny: vec![],
                },
            ),
            (
                "strict-unrestricted".to_string(),
                ProfileConfig {
                    extends: Some("strict".to_string()),
                    restrict_network: Some(false),
                    read_only: vec![],
                    read_write: vec![],
                    deny: vec![],
                },
            ),
            (
                "workspace-restricted".to_string(),
                ProfileConfig {
                    extends: Some("workspace".to_string()),
                    restrict_network: Some(true),
                    read_only: vec![],
                    read_write: vec![],
                    deny: vec![],
                },
            ),
        ]),
    }
}
