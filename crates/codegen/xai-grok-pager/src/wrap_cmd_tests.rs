//! Unit tests for the `grok wrap` spawn planning in [`super`] (`wrap_cmd`),
//! split out via `#[path]` to keep the module itself small.
//!
//! Everything is pure — `derive_spawn` takes the PATH lookup result and the
//! shell as inputs — except the final test, which round-trips the rejoined
//! command line through a real `/bin/sh -c` to prove the quoting contract end
//! to end.

use super::*;
use pretty_assertions::assert_eq;

fn cmd(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

#[test]
fn single_arg_with_whitespace_always_routes_via_shell() {
    // Wins over PATH resolution: the arg is a command line, not a program.
    let plan = derive_spawn(
        &cmd(&["mycli ssh host-01"]),
        "/bin/zsh",
        true,
        ShellMode::Interactive,
    );
    assert_eq!(plan.program, "/bin/zsh");
    assert_eq!(plan.args, cmd(&["-i", "-c", "mycli ssh host-01"]));

    // Wins over the explicit-path rule too: a single program path with
    // spaces word-splits in the shell (quote it for the shell instead).
    let plan = derive_spawn(
        &cmd(&["/path/with space/prog"]),
        "/bin/zsh",
        false,
        ShellMode::Interactive,
    );
    assert_eq!(plan.args, cmd(&["-i", "-c", "/path/with space/prog"]));

    // Any whitespace counts, not just spaces: tabs/newlines also mark the
    // arg as a command line to hand to the shell verbatim.
    let plan = derive_spawn(
        &cmd(&["x\tssh\nhost"]),
        "/bin/sh",
        false,
        ShellMode::Interactive,
    );
    assert_eq!(plan.program, "/bin/sh");
    assert_eq!(plan.args, cmd(&["-i", "-c", "x\tssh\nhost"]));
}

#[test]
fn resolvable_program_spawns_directly() {
    let plan = derive_spawn(
        &cmd(&["explorer", "ssh", "host"]),
        "/bin/zsh",
        true,
        ShellMode::Interactive,
    );
    assert_eq!(plan.program, "explorer");
    assert_eq!(plan.args, cmd(&["ssh", "host"]));

    // A single resolvable word is a plain program run, not an alias.
    let plan = derive_spawn(&cmd(&["ls"]), "/bin/zsh", true, ShellMode::Interactive);
    assert_eq!(plan.program, "ls");
    assert_eq!(plan.args, Vec::<String>::new());

    // Explicit paths never route through the shell, resolvable or not —
    // relative or absolute.
    let plan = derive_spawn(
        &cmd(&["./run.sh", "arg"]),
        "/bin/zsh",
        false,
        ShellMode::Interactive,
    );
    assert_eq!(plan.program, "./run.sh");
    let plan = derive_spawn(
        &cmd(&["/abs/prog", "arg"]),
        "/bin/zsh",
        false,
        ShellMode::Interactive,
    );
    assert_eq!(plan.program, "/abs/prog");

    // A multi-arg first word with whitespace is never an alias: keep the
    // direct spawn and its precise not-found error.
    let plan = derive_spawn(
        &cmd(&["my prog", "arg"]),
        "/bin/zsh",
        false,
        ShellMode::Interactive,
    );
    assert_eq!(plan.program, "my prog");
    assert_eq!(plan.args, cmd(&["arg"]));

    // Same for an empty first word (unset `$PROG`): fail fast on the
    // direct spawn rather than shell-executing the tail.
    let plan = derive_spawn(
        &cmd(&["", "rm", "-rf", "x"]),
        "/bin/zsh",
        false,
        ShellMode::Interactive,
    );
    assert_eq!(plan.program, "");
}

#[test]
fn direct_route_passes_args_verbatim_without_quoting() {
    // Quoting exists only on the shell route's rejoined line; a resolvable
    // program must receive its argv elements byte-for-byte.
    let plan = derive_spawn(
        &cmd(&["rsync", "a b", "don't"]),
        "/bin/zsh",
        true,
        ShellMode::Interactive,
    );
    assert_eq!(plan.program, "rsync");
    assert_eq!(plan.args, cmd(&["a b", "don't"]));
}

#[test]
fn alias_route_keeps_first_word_bare_and_quotes_the_tail() {
    // First word stays unquoted (aliases only expand on bare words); every
    // tail word is quoted — zsh expands a bare `=word`, so no safe set.
    let plan = derive_spawn(
        &cmd(&["x", "ssh", "=host", "don't"]),
        "/bin/zsh",
        false,
        ShellMode::Interactive,
    );
    assert_eq!(plan.program, "/bin/zsh");
    assert_eq!(plan.args, cmd(&["-i", "-c", "x 'ssh' '=host' 'don'\\''t'"]));

    // Bare alias with no args: the most common invocation.
    let plan = derive_spawn(&cmd(&["x"]), "/bin/zsh", false, ShellMode::Interactive);
    assert_eq!(plan.args, cmd(&["-i", "-c", "x"]));
}

#[test]
fn plain_shell_mode_drops_dash_i() {
    let plan = derive_spawn(
        &cmd(&["mycli ssh host"]),
        "/bin/sh",
        false,
        ShellMode::Plain,
    );
    assert_eq!(plan.program, "/bin/sh");
    assert_eq!(plan.args, cmd(&["-c", "mycli ssh host"]));
}

#[test]
fn quote_word_edge_cases() {
    assert_eq!(quote_word("don't"), "'don'\\''t'");
    assert_eq!(quote_word(""), "''");
    // A lone quote is the worst case for the close-escape-reopen idiom.
    assert_eq!(quote_word("'"), "''\\'''");
}

#[test]
fn quote_word_neutralizes_shell_metacharacters() {
    // Expansion/control characters must come out single-quoted inert.
    for meta in ["$HOME", "`id`", "*", "!!", ";", "&&", "|", "a\nb"] {
        assert_eq!(quote_word(meta), format!("'{meta}'"));
    }
}

#[test]
fn join_command_line_shapes() {
    // Zero tail words: just the bare first word, no trailing space.
    assert_eq!(join_command_line(&cmd(&["x"])), "x");
    // An empty tail word survives as an explicit empty argument.
    assert_eq!(join_command_line(&cmd(&["x", ""])), "x ''");
}

#[test]
fn resolve_shell_falls_back_to_bin_sh() {
    assert_eq!(resolve_shell(None), "/bin/sh");
    assert_eq!(resolve_shell(Some("")), "/bin/sh");
    assert_eq!(resolve_shell(Some("/nonexistent/shell-xyz")), "/bin/sh");
}

#[test]
fn resolve_shell_uses_existing_file() {
    let file = tempfile::NamedTempFile::new().expect("tempfile");
    let path = file.path().to_str().expect("utf8 path").to_string();
    assert_eq!(resolve_shell(Some(&path)), path);
}

/// Feed the rejoined command line through a real `/bin/sh -c` and assert the
/// child receives exactly the original words — the quoting contract proven
/// against an actual shell, not just against expected strings.
#[test]
fn joined_line_roundtrips_words_through_real_sh() {
    let words = [
        "a b",
        "don't",
        "=x",
        "new\nline",
        "$HOME",
        "`id`",
        "*",
        ";",
        "&&",
        "|",
        "",
    ];
    // `printf` repeats the format per argument, so each word renders as `[w]`.
    let mut command = vec!["printf".to_string(), "[%s]".to_string()];
    command.extend(words.iter().map(|w| w.to_string()));

    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(join_command_line(&command))
        .output()
        .expect("run /bin/sh");

    assert!(
        out.status.success(),
        "sh rejected the joined line: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected: String = words.iter().map(|w| format!("[{w}]")).collect();
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn with_ssh_env_forwarding_injects_send_env_for_ssh() {
    let plan = SpawnPlan {
        program: "ssh".to_string(),
        args: vec!["user@host".to_string()],
    }
    .with_ssh_env_forwarding();
    assert_eq!(plan.program, "ssh");
    assert_eq!(
        plan.args,
        vec![
            "-o".to_string(),
            "SendEnv=LC_GROK_OSC52_SINK".to_string(),
            "-o".to_string(),
            "SendEnv=LC_GROK_APPEARANCE".to_string(),
            "user@host".to_string(),
        ]
    );
}

#[test]
fn with_ssh_env_forwarding_preserves_user_supplied_o_flags() {
    // User may already have -o SendEnv=FOO or other options; ours go first
    // (so they're in ssh's own option block, before the hostname) and the
    // user's tail is untouched.
    let plan = SpawnPlan {
        program: "/usr/bin/ssh".to_string(),
        args: vec![
            "-p".to_string(),
            "2222".to_string(),
            "-o".to_string(),
            "SendEnv=FOO".to_string(),
            "user@host".to_string(),
            "--".to_string(),
            "tmux".to_string(),
        ],
    }
    .with_ssh_env_forwarding();
    assert_eq!(plan.program, "/usr/bin/ssh");
    assert_eq!(
        &plan.args[..6],
        &[
            "-o",
            "SendEnv=LC_GROK_OSC52_SINK",
            "-o",
            "SendEnv=LC_GROK_APPEARANCE",
            "-p",
            "2222",
        ]
    );
    assert_eq!(
        &plan.args[6..],
        &["-o", "SendEnv=FOO", "user@host", "--", "tmux"]
    );
}

#[test]
fn with_ssh_env_forwarding_skips_non_ssh_programs() {
    // SendEnv is ssh-specific; passing it to docker/kubectl/autossh would
    // error.
    for program in ["docker", "kubectl", "autossh", "sshpass", "sshfs"] {
        let plan = SpawnPlan {
            program: program.to_string(),
            args: vec!["exec".to_string(), "-it".to_string()],
        }
        .with_ssh_env_forwarding();
        assert_eq!(
            plan.args,
            vec!["exec".to_string(), "-it".to_string()],
            "{program} must not receive SendEnv flags"
        );
    }
}

#[test]
fn with_ssh_env_forwarding_recognizes_windows_ssh_exe() {
    let plan = SpawnPlan {
        program: r"C:\Windows\System32\OpenSSH\ssh.exe".to_string(),
        args: vec!["user@host".to_string()],
    }
    .with_ssh_env_forwarding();
    assert_eq!(plan.program, r"C:\Windows\System32\OpenSSH\ssh.exe");
    assert_eq!(
        plan.args[0..2],
        ["-o".to_string(), "SendEnv=LC_GROK_OSC52_SINK".to_string()],
        "windows ssh.exe must be detected by basename"
    );
}

#[test]
fn with_ssh_env_forwarding_does_not_splice_into_shell_routed_plan() {
    // When the user runs `grok wrap "ssh host"`, derive_spawn routes through
    // $SHELL -i -c 'ssh host' (alias expansion). We deliberately do not
    // splice -o into the shell's argv: the quoted command line is the
    // shell's job to parse, and trying to inject inside it would corrupt
    // quoting. The plan returned by derive_spawn for that case is what
    // should reach run_wrapped_command.
    let plan = SpawnPlan {
        program: "/bin/zsh".to_string(),
        args: vec!["-i".to_string(), "-c".to_string(), "ssh host".to_string()],
    }
    .with_ssh_env_forwarding();
    assert_eq!(plan.program, "/bin/zsh");
    assert_eq!(plan.args, vec!["-i", "-c", "ssh host"]);
}

#[test]
fn program_is_ssh_matches_canonical_and_pathed_forms() {
    assert!(program_is_ssh("ssh"));
    assert!(program_is_ssh("/usr/bin/ssh"));
    assert!(program_is_ssh("/usr/local/bin/ssh"));
    assert!(program_is_ssh("./ssh"));
    assert!(program_is_ssh("SSH"));
    assert!(program_is_ssh("Ssh"));
    // Windows
    assert!(program_is_ssh("ssh.exe"));
    assert!(program_is_ssh(r"C:\Windows\System32\OpenSSH\ssh.exe"));
    assert!(program_is_ssh("SSH.EXE"));
    // Negatives
    assert!(!program_is_ssh("sshpass"));
    assert!(!program_is_ssh("sshfs"));
    assert!(!program_is_ssh("autossh"));
    assert!(!program_is_ssh("docker"));
    assert!(!program_is_ssh(""));
}
