# Contributing

This repository does **not** accept external pull requests or unsolicited
patches.

SpaceXAI develops this software internally. The public tree is published for
source transparency and local builds under the terms of the Apache License,
Version 2.0 (see [`LICENSE`](LICENSE)).

## Local setup

Install the repository hooks once per clone:

```sh
./scripts/install-hooks.sh
```

This sets `core.hooksPath` to `scripts/hooks`, enabling a pre-commit scan that
refuses credential material and local-machine artifacts (environment dumps,
`.pem`/`.env` files, literal Windows temp paths). The same scan runs in CI, so
skipping the hook only moves the failure later. A commit was once pushed to the
public remote carrying live API keys; the hook exists so that cannot recur.

For an intentional fixture containing a fake key, annotate the line with a
`secret-scan:allow` comment (on the line or the one above it) rather than
disabling the scan.

New Rust tests should use `#[ignore = "reason; review YYYY-MM"]` only when an
external service or platform is genuinely required. Bare `#[ignore]` entries
are grandfathered by `scripts/ci/ignored-tests-baseline.tsv`; additions and
removals both require a reviewed baseline change. The quarterly audit still
requires a clear reason and review date.

## Low-memory builds

This repository's Rust workspace has a large build graph and can use substantial
memory, disk space, and filesystem metadata during compilation. On WSL2 or other
low-memory machines, limit Cargo parallelism rather than running several Cargo
builds at once:

```sh
CARGO_BUILD_JOBS=4 cargo check --workspace --all-targets --locked
CARGO_BUILD_JOBS=4 cargo test --workspace --locked
```

Avoid running release and check builds concurrently on a memory-constrained WSL2
checkout. The `target/` directory can grow large; inspect its size before a full
rebuild, and remove only disposable `target/debug` or incremental artifacts when
space is needed. Preserve source, caches outside `target/`, and user data.

The observed WSL2 9P shutdown incident and its environment-specific evidence are
recorded in [`docs/known-issues/wsl-p9io-crash-20260728.md`](docs/known-issues/wsl-p9io-crash-20260728.md).

## Clean-container verification

A passing local run is not proof that the documented steps work: a warm
`target/`, a globally installed `protoc`, a hand-installed toolchain or an OS
trust store that already has the right roots can each hide a step that a fresh
clone would need. `scripts/verify-in-docker.sh` builds
[`docker/verify.Dockerfile`](docker/verify.Dockerfile) and runs the same command
list as the `rust` job in `.github/workflows/ci.yml` inside it:

```sh
scripts/verify-in-docker.sh          # fmt, guard scripts, check, clippy
scripts/verify-in-docker.sh --full   # the above plus cargo test --workspace
scripts/verify-in-docker.sh --shell  # same image, interactive
```

The image installs its toolchain through rustup from `rust-toolchain.toml` and
asserts `rustc -V` against that pin at build time, so the pin stays the only
source of truth and a silent fallback to another compiler fails the build.
Cargo's registry and `target/` live in named volumes rather than the bind mount,
so a root-owned build tree cannot break the host build afterwards. `RUST_MIN_STACK`
is set to the value both CI jobs set it to, because `xai-grok-shell` has actor tests
that overflow the harness default and the lab's claim is that it runs CI's command
list.

The build context is a second thing to keep honest. The image itself reads one
file, `rust-toolchain.toml`, and the tree arrives later as a bind mount, so
`.dockerignore` is an allow list (`*`, plus `!rust-toolchain.toml`) rather than a
list of exclusions. Without it `docker build` uploads the whole working tree,
which on a day's development is hundreds of gigabytes of `target/` before it
reads those 740 bytes. A `COPY` added to the Dockerfile therefore needs a matching
`!` line; forgetting one fails the build with `not found` instead of silently
producing a thinner image.

The run checksums every tracked and untracked file before the first gate and again
after the last one. A mismatch prints `UNATTRIBUTABLE` and exits non-zero: the
container reads the live working tree, so a run that overlapped an edit describes
neither a commit nor a clean tree, and a `cargo test` leg that raced an editor looks
exactly like a genuine failure. Run it with the tree at rest.

Behind a registry mirror, point the build at your own base image:

```sh
BASE_IMAGE=your-mirror.example.com/library/debian:bookworm-slim scripts/verify-in-docker.sh
```

This container covers the Linux gates only. It is not platform evidence: a Linux
container cannot run the macOS or Windows code paths, and it does not exercise
signing, installers, or a real TLS-terminating deployment.

It also runs part of `scripts/ci/`, and that part is now a rule rather than a
habit. `scripts/ci/check-guard-wiring.py` classifies every guard as **mirrored**
(the `gates` array names it, or a script the entry point runs does) or **CI-only**,
and a guard in neither set fails the check. The CI-only set lives in
`scripts/ci/docker-entry-ci-only.tsv`, one `<name><TAB><reason>` row per guard, and
the reasons are all the same shape: something a clean container off this repository
does not have. Today that is a dev-profile build of `chaos-engine`
(`check-gui-protocol.sh`), a real PowerShell (`check-powershell-syntax.py --require`),
and assembled release artifacts (the release-integrity lab helpers). `--list-mirror`
prints the classification. When you add a guard, add it to the `gates` array or add
a row; adding it to CI alone is the one option the check refuses, because that is
how a local run quietly stops covering what it used to.

Mirroring says both places run a guard; it does not say they ask the same thing of
it. That was the shape of the first drift the rule caught: the four budgets of
`platform-gated-tests.py` are written at both call sites, one lowering edited one of
the two files, and the leg nobody runs locally went on enforcing a looser cap while
every check stayed green. The same check therefore compares flag values between the
entry point and each workflow. A flag only one side passes stays legal, because
`--require` is deliberately Windows-leg-only and the container cannot satisfy it, but
a flag both sides pass has to carry the same values in both -- which means lowering a
budget is a one-commit change to two files.

### Commands you write down are read

`scripts/ci/check-evidence-commands.py` reads every command line in the repository:
a line starting with `$ ` in any tracked document or in `docs/verification/*.log`,
plus every line of a fence labelled `sh`, `bash`, `console` or `pwsh`. Two rules
apply to all of them.

A named interpreter has to be able to parse the file it is handed. `python3` gets a
`.py`, `bash` gets a `.sh`, `pwsh` gets a `.ps1`, `node` gets a `.js`/`.mjs`/`.cjs`.
This is not pedantry: `python3 scripts/ci/check-versions.sh` does not stop a shell
script, it runs the fragments it happens to recognise, and if the exit code is taken
from the last stage of a pipeline the failure never surfaces. Paste-able reproducers
say which interpreter they use. A name inside quotes is not a command, so
`printf '%s\n' 'python3 scripts/ci/x.sh'` says nothing about python3.

A path a command names has to still exist, unless the command is what creates it or
the token is shaped like a pattern. Creators are `mkdir`, `touch`, `rm`, `rmdir`,
`tee`, `truncate` and a redirect target; `cp`, `mv` and `install` are read as what they
are, so the file being copied out of has to be there while the file being written to
does not. A transcript that names a retired file is a recipe that reproduces nothing,
even when it is an honest record of a run.

Where the line should stay as written, add a row to
`scripts/ci/evidence-commands-allowlist.tsv`: `<key><TAB><category><TAB><reason>`. The
key is what the finding names, which is the path for `historical`, `recorded-absent` and
`other-root`, and the whole command for `quoted-command`. That last category exists
because a document sometimes has to print a broken command to show what was wrong with
it, and a quotation of a mistake looks exactly like a recommendation unless the row says
otherwise. A row whose finding has gone away fails as stale, so fixing the line means
deleting the row rather than leaving it behind.

`scripts/ci/check-doc-path-refs.py` is the companion gate: it reads the paths themselves
rather than the commands around them, and a document may not name a file the repository
does not have. For a change with an evidence log that fixes the order of operations,
because `CHANGELOG.md` and `TODO.md` cite the log by path -- so write the log first.
Citing it early reports the failure against the citing line instead:

```
check-doc-path-refs: CHANGELOG.md:29: `docs/verification/pipefail-report-gate-2026-10-04.log` resolves to nothing in the repository (unrecorded)
```

A doc worded correctly then looks like the broken part, and the file that has not been
written yet is never named as the cause.

### A metric call with the wrong number of labels aborts the process

`with_label_values` is the neat form of a Prometheus lookup, and it is implemented as an
unwrap of the checked one: in `prometheus` 0.14.0, the version the lockfile pins, the function
is at `src/vec.rs:292`, the `unwrap()` that panics is at `src/vec.rs:296`, and the length check
that fails first is at `src/vec.rs:118`. One extra or one missing entry in the array is
therefore an abort at the call site, and the three things that would normally catch a mistake
do not see it. The compiler cannot: the labels are a runtime slice, and `&[&str]` of any
length typechecks against `&[V] where V: AsRef<str>`. `panic-site-census.py` cannot: it counts
panic-capable sites by the tokens that cause them, and no `unwrap`, `expect`, `panic!` or
`unsafe` token sits at the call site, because the panic lives inside the dependency. A test
sees it only where a test reaches that call, and 101 of the 155 label-value call sites here
are in production code, on startup, drain, recovery, swap and OOM paths.

`scripts/ci/check-metric-labels.py` reads both sides of the pairing. On the registration side
it checks the 83 metric registrations, 82 through the `register_*!` macros and one built by
hand: the metric name is a valid Prometheus name, no name is registered twice on the default
registry, and label names are valid and unique inside one metric. All 82 macro registrations
consume the `Result` inside a `LazyLock`, 36 by `unwrap` and 46 by `expect`, so a duplicate
name aborts in whatever code first touches the metric rather than where it was registered, and
this gate is the reason those calls are safe. On the call side it checks that each
`with_label_values`, `get_metric_with_label_values`, `remove_label_values` and
`delete_label_values` call passes as many values as the metric it names declares. The first
aborts on a mismatch; the other three return the error, so a mismatch there is a metric that
silently stops reporting. Both are findings.

The receiver is matched by name against the `static`, `const` or `let` a registration is bound
to, which is exact for the `static` behind a `LazyLock` form this tree uses everywhere; an
identifier used for two metrics with different label counts is reported as ambiguous rather
than resolved by guessing. What the gate cannot judge is a finding and never a pass: a label
list that is not an array literal is `dynamic-labels`, a receiver reached through a field or a
function call is `unresolved-receiver`. Where one of those is correct as written, a row in
`scripts/ci/metric-labels-allowlist.tsv` records it, and a row whose finding has gone away
fails as stale.

### A timeout that abandons a child process leaves the child running

tokio spawns the child inside `Command::output()` and `Command::status()` themselves
(`tokio-1.52.3/src/process/mod.rs:1069` and `:1003`), and the module documentation is explicit
about what happens next, at `:201-203`: "unlike the futures paradigm of
dropping-implies-cancellation, a spawned process will, by default, continue to execute even
after the `Child` handle has been dropped". `kill_on_drop` is what changes that, and the
default is off (`:641`). So `tokio::time::timeout(budget, cmd.output())` cancels the waiting
and not the work, unless somebody put the flag on `cmd`.

Nothing else sees the omission. The compiler cannot: `kill_on_drop` is an ordinary `&mut self`
builder method, and leaving a builder method out of a chain has never been a type error. The
lint that looks like it covers this cannot either: `clippy.toml` bans `std::process::Command::spawn`
and `tokio::process::Command::spawn`, and `spawn` is the one call that has an alternative --
`ProcessScope::enroll` takes the `&Child` it returns -- while `.output()` and `.status()` never
hand out a handle to enroll. A test that takes the timeout asserts on the timeout, so it goes
green while the child it abandoned keeps running. `detach_command` makes an unmarked site worse
rather than better, because it `setsid`s the child into its own session, so once the future is
dropped there is no process group left that the parent's teardown could signal.

`scripts/ci/check-timeout-child.py` judges only the shape where the code itself schedules the
drop: a `timeout(...)` call handed a future that ends in `.output()` or `.status()` on a
`tokio::process::Command`. The path from `Command::new` to the call is read three ways: the
method chain, a local binding plus the statements that touch it afterwards, and a builder or
mutator function whose body is in the tree. Helper names resolve inside the calling crate
first, because `git_command` alone is defined in three crates here and two of them build a
`std::process::Command`, which the rule does not apply to: its `.output()` blocks until the
child exits, so there is no future to walk away from, and those sites are counted separately.
What the gate cannot read is a finding and never a pass (`unreadable-future`,
`unreadable-receiver`, `unknown-helper`, `ambiguous-command`); where one of those is correct as
written, a row in `scripts/ci/timeout-child-allowlist.tsv` records it and fails as stale once
the finding it excuses is gone.

The scanner found the rule by being run once. Of the 12 timeout-abandoned sites on this tree,
10 already killed their child and 2 did not, behind an identical call shape, and nothing
outside the script distinguished the two groups. Both are marked now, and
`baseline_capture_timeout_kills_the_git_it_abandoned` in
`xai-grok-shell/src/session/goal_classifier_tests.rs` proves the flag is what stops the
process: it points `GIT_BIN_PATH` at a shim that records its own pid and then blocks, and
asserts the pid stops existing once the capture budget expires. Take the flag out and that
test goes red, which is the difference between this gate and a grep.

## Fast local gate loop

The container answers "does a fresh clone work?". It does not answer "did my edit
break a guard?", because the same list also runs `cargo check` and `cargo clippy` over
the whole workspace, which is an hour for a ten-line change.
`scripts/verify-gates.sh` runs that same gate list on the host. The list is parsed out
of the `gates` array of `scripts/verify-in-docker.sh` instead of copied, so a gate added
there appears here with the same label and the same command line, and neither copy can
drift from the other.

```sh
scripts/verify-gates.sh              # the cheap gates, in array order
scripts/verify-gates.sh --list       # what would run, running nothing
scripts/verify-gates.sh --only fmt   # only the gates whose label contains "fmt"
scripts/verify-gates.sh --verbose    # stream every gate's output, not just failures
scripts/verify-gates.sh --with-build # plus cargo check/clippy/test and GUI types
scripts/verify-gates.sh --self-test  # the runner's own fixture suite
```

Two differences from the container run are deliberate, and the runner prints both in its
header. The `gates` entries' `${bootstrap}` prefix (git `safe.directory` plus an
identity) is dropped: the container needs it because the tree is bind-mounted from a
foreign uid, and a test runner has no business rewriting a contributor's global git
config. The four build gates are skipped unless `--with-build`. Everything else runs
verbatim, including the `rustup target add` inside the load-bearing-feature guard, which
is what makes that guard fail rather than silently skip a target.

`--self-test` is itself an entry in the `gates` array, so the container runs it too. It
pins the shapes that would otherwise rot quietly: the appended `gates+=(...)` entries of
full mode are found, a failing gate is named and changes the exit code, and a source the
extractor cannot parse exits 2 instead of reporting an empty pass.

`--only <label>` filters the list, repeatable, matching a label exactly or as a fragment. It
exists because the expensive question is usually about one gate: the lint set that the host
run only applies under `--with-build` can be asked of the container directly with
`scripts/verify-in-docker.sh --only 'cargo clippy'`, which costs about three minutes with warm
cargo volumes instead of the 23 to 25 minutes of a `--full` sweep. Three rules keep a filtered
run from being quoted back as a sweep. A pattern matching no label exits 2 and names the
pattern, so a typo cannot select nothing and print a verdict about nothing. A filtered summary
carries the counts (`K of T selected by --only` on the host, `--only was in effect: K of M
gates ran` in the container), while an unfiltered host sweep says
`all gates passed on the host (30 run, 4 skipped)` and only that. And the filter does not
unlock the build gates: `--only 'cargo test'` on the host still prints `SKIP` unless
`--with-build` is also given, because a fragment that happens to match a build gate should not
turn a fast loop into a workspace rebuild. `--self-test` pins all three, including the negative
half of each selection case, which asserts that the line of the gate that must not have run is
absent.

The other labs (`install-sh-in-docker.sh`, `install-integrity-in-docker.sh`,
`npm-install-in-docker.sh`, `remote-acceptance-in-docker.sh`) start their containers
idle and drive every step with `docker exec`, so each container's lifetime is whatever
idle command it was started with. Keep two things in mind when you touch that. The idle
command needs a ceiling: if the lab is killed hard enough that its cleanup trap never
runs, a container with no ceiling stays on the machine forever. And the ceiling needs to
outlast a slow but successful run — when it expires the containers stop underneath
whichever check is in flight, and the lab reports a killed process instead of a
verdict. `install-sh-in-docker.sh` downloaded a 150 MB release for an hour, hit exactly
that, and blamed `install.sh` for exiting 137. All four now take `CHAOS_LAB_KEEPALIVE`
(default six hours) and print the value they are using.

## Platform-specific checks

Several crates select behaviour by `cfg(unix)` / `cfg(windows)` /
`cfg(target_os = ...)`: the TTY stderr-handle handling, the sandbox's seccomp and
namespace paths versus the Windows Job Object paths, PTY descendant teardown, the
updater's per-OS installer hint, and child-process spawning. A green Linux run
says nothing about those paths, so run the target machine's own entry point:

```sh
# macOS / Linux
scripts/test-platform.sh 2>&1 | tee "platform-test-$(uname -s).log"
```

```powershell
# Windows (PowerShell)
./scripts/test-platform.ps1 *>&1 | Tee-Object platform-test-windows.log
```

Both print the toolchain/OS report first and then run the same crate set the
`platform-tests` CI job uses (`macos-14` and `windows-latest`), and both accept
crate names to narrow the run. Keep the log: it is the evidence a platform row in
`TODO.md` needs, and a Linux log never substitutes for it.

When a run fails, send the whole log rather than the last few lines. The header
the scripts print (OS build, `rustc -Vv`, logical CPU count, `RUST_MIN_STACK`,
commit, and a `working tree : dirty` marker) is what tells a reader whether the
failure is the checked-in code or a local edit, so a truncated tail usually
cannot be diagnosed.

### Shell script portability

`scripts/test-platform.sh` and the other scripts under `scripts/` run on the
contributor's machine, not only on the Linux CI runner. macOS still ships
bash 3.2 and BSD userland, so `mapfile`, `readarray`, `declare -A`, `${var^^}`,
GNU `sed -i`, `nproc`, `readlink -f`, `grep -P`/`--include` and `timeout` all
fail there with a bare "command not found" or a silently half-done file.
`scripts/ci/check-script-portability.py` rejects those constructs, and it has no
allow list on purpose: if a rule fires, the script gets rewritten.
`scripts/ci/test-script-portability.py` injects one violation per rule and
asserts the check still exits 1, because a scanner that stopped matching looks
exactly like a repository that was fixed.

### Shell scripts that stop before they report

The other way a script in `scripts/` goes wrong without saying so. Under
`set -e`, a plain `name="$(pipeline)"` assignment takes its exit status from the
command substitution, so the script ends at the assignment line. That is right for
a command that failed, and wrong for one whose non-zero status is an ordinary
answer: `grep` exits 1 when nothing matched, `diff` exits 1 when the files differ,
`wc` exits non-zero when the file it was asked to measure is not there. Three
shipped scripts did this on 2026-10-04, and in every one of them the exit code was
already correct while the report was gone, because the code that prints the report
sat one statement later.

`scripts/ci/check-pipefail-report.py` rejects that assignment shape for those
commands; the fix is a command without the extra status (`sed '/^$/d'` rather than
`grep -v '^$'`, `awk 'NF { n += 1 } END { print n + 0 }'` rather than `grep -c .`)
or an explicit `|| true` that keeps the handling below reachable. Like the
portability check it has no allow list. It skips `local`/`export` assignments,
which mask the status instead of propagating it, and it reads quoted strings as
text, so a `| grep` inside a printed message is not a pipeline. A script has to
turn `-e` on to be in scope at all: `set -uo pipefail` produces the same non-zero
status and leaves nobody to act on it, which is why the host gate runner
(`scripts/verify-gates.sh`, and it needs that, because it aggregates gate failures)
is not scanned.

`scripts/ci/test-check-pipefail-report.py` keeps both directions honest: the three
lines that shipped are in it verbatim, so they stay test cases, and near misses
(`ROOT="$(cd "$(dirname "$0")" && pwd)"`, a heredoc body writing a `grep`, a
message containing `|| true`) have to stay quiet. A gate that cries wolf on those
gets switched off, which is how the three survived as long as they did.

## Remote workspace sessions

Two binaries carry the remote-workspace transport
(`crates/codegen/chaos-engine/src/remote/`):

```sh
# On the host that owns the workspace. It sees exactly one directory.
chaos-remote-server --workspace ~/src/beeper --tcp 127.0.0.1:7788 \
  --token-file ~/.chaos/remote-tokens --capability tool-execution --allow cargo

# On the machine doing the work, through a tunnel whose local end is loopback.
chaos-remote --tcp 127.0.0.1:17788 --token-file ~/.chaos/remote-tokens list src
chaos-remote --tcp 127.0.0.1:17788 --token-file ~/.chaos/remote-tokens \
  exec --timeout 600 -- cargo test -p beeper
chaos-remote --tcp 127.0.0.1:17788 --token-file ~/.chaos/remote-tokens \
  install 0.4.0 --from target/release/chaos-remote-server

# Through a tunnel that is still coming up, with a bound on being stuck.
chaos-remote --tcp 127.0.0.1:17788 --token-file ~/.chaos/remote-tokens \
  --connect-wait 30 --reply-timeout 120 ping
```

A service on the remote host can be reached from here the way `ssh -L` reaches it.
The server decides which services are reachable at all, by address and port, and
naming one is the only way to enable forwarding:

```sh
# On the remote host: forward to the review app it can already reach.
chaos-remote-server --workspace ~/src/beeper --tcp 127.0.0.1:7788 \
  --token-file ~/.chaos/remote-tokens --allow-forward-to 127.0.0.1:3000

# On the machine doing the work: a local loopback port, gone when the grant is.
chaos-remote --tcp 127.0.0.1:17788 --token-file ~/.chaos/remote-tokens \
  forward --to 127.0.0.1:3000 --listen 0
```

Three properties are deliberate and are what the tests around this code exist
to hold:

- **Loopback only.** Both `--tcp` forms refuse a routable address at parse time.
  The tunnel carries the trust; the transport does not attempt TLS, and a
  `ssh -L`-style tunnel is assumed rather than implemented here. The local end of a
  forward is held to the same rule, because a forwarded service on a routable
  interface is a published service.
- **One-time credentials.** Each credential opens exactly one session. Reuse is
  refused as reuse, a credential past its TTL is refused as expired, and the
  client removes the credential it spent from the file it was given.
- **Nothing is faked.** `--capability interactive-pty` and `detached-agent` are
  refused with a reason, and `exec` runs only programs named by `--allow`.
- **An installed artifact has to say who signed it and which machine it is for.**
  `install` sends the artifact with its release sidecar — `--signature FILE`, or
  `FILE.sig` found beside the artifact when that flag is absent — and the host checks,
  in that order and all of it before the first `mkdir`: the sha256 of the bytes it
  actually received, then an ed25519 signature from a key *that host* was told about
  (`--trust-signing-key <base64|@file>` or `CHAOS_SIGNING_PUBLIC_KEY`), then the file's
  own ELF / Mach-O / PE header against the host's OS and architecture. A host with no
  key configured refuses every install rather than accepting whatever arrives;
  `--allow-unsigned-artifact` or `CHAOS_REMOTE_REQUIRE_SIGNATURE=0` opts out of the
  requirement, not out of checking a signature that is offered. A host says which of
  these it is doing at startup, on its `artifacts: …` line. Every refusal names its
  reason — `signature_missing`, `signature_malformed`, `signature_invalid`,
  `no_trusted_key`, `artifact_too_large`, `wrong_platform` — and publishes nothing: no
  version directory, no moved pointer, no half-written upload left in the install root.
  What it does not buy: provenance is not a permission. An artifact that passes all
  three checks still only does what the session's capabilities allow, and `exec` stays
  behind `--allow`.
- **Forwarding is the operator's call, not the session's.** A server with no
  `--allow-forward-to` has nowhere a forward may go and refuses to start when asked
  to advertise the capability anyway. A forward is authorised by a *ticket* kept in
  its own vault, bound to one `host:port`, valid for a bounded number of
  connections, and withdrawn when the session that asked for it closes; it opens no
  session and reads nothing. Remote forwarding (`ssh -R`, the server listening on
  the client's behalf) is not implemented and is refused by name.
- **Both waits are bounded, and they are different waits.** `--connect-wait` re-dials
  for as long as it is told — the delay doubles up to 2s — and repeats *only* the dial,
  so waiting for a tunnel that is still coming up can never burn a credential.
  `--reply-timeout` bounds a reply instead: the handshake's and each request's. When a
  request's reply does not arrive, the session ends rather than pauses
  (`RemoteWorkspace::state()` reports `Abandoned`), because a late reply would answer
  the wrong question and a request cannot be unsent. Nothing reconnects by itself, on
  purpose: a credential is one-time, so reconnecting means a new session and a new
  credential, which is a decision for whoever runs the command rather than a background
  retry. A credential also leaves the token file the moment it is sent — including on a
  run that dies in the handshake — so a failed run cannot poison the next one.

`scripts/remote-acceptance-in-docker.sh` is the gate for all of the above. It
builds the artifacts here, then puts the server in a stock Debian container that
has never seen this repository, reaches it through a `socat` tunnel, and checks
reading, searching, writing, `git diff`, tool execution, path-escape refusals,
credential handling, a full disk, an artifact on a `noexec` filesystem, a dropped
connection, and both clocks: a late reply, a tunnel that appears late, and a peer
that accepts the connection and never speaks. Provenance is checked the same way and
with keys generated inside the run: an unsigned artifact is refused by a host holding
a key, the same bytes install once their sidecar is there, a signature from a key the
host was never told about is refused, bytes changed after signing are refused although
their digest is correct, a Mach-O arm64 header is refused on a Linux x86-64 host, a
keyless host refuses both an unsigned artifact and a correctly signed one, and a host
that opted out of the requirement still refuses a bad signature. Forwarding is checked the same way: a
`python3 -m http.server` on the remote host is fetched through a forward from the
other container and the digest compared against the file on that host's disk, an
unlisted target is refused although something really is listening on it, a grant of
two connections is seen to end the forward and release the local port by itself, and
a hand-written RFC 6455 service on the remote host completes a WebSocket handshake
through the tunnel and returns a file the developer container cannot read.

```sh
scripts/remote-acceptance-in-docker.sh 2>&1 | tee "remote-acceptance-$(date +%F).log"
```

A run leaves its transcript in the log you tee it to;
[`docs/verification/remote-acceptance-linux-2026-10-02.log`](docs/verification/remote-acceptance-linux-2026-10-02.log)
is a kept example. The lab is Linux-only and there is no SSH transport in this
build, so it is not evidence about SSH host-key handling or about a macOS or
Windows remote host.

## Serving the Web host behind TLS

`chaos-web` binds `127.0.0.1` and stays that way: a TLS terminator in front of it
is the deployment, not a `--bind 0.0.0.0` flag. Because the proxy dials loopback,
it has to share the backend's network namespace (a sidecar pod, or
`docker run --network container:<web>`), and it forwards headers the backend has
never seen from a local browser:

- `Host: chaos.example.com` — refused unless you declare it, because the Host
  check is what stops DNS rebinding against a loopback server.
- `Origin: https://chaos.example.com:8443` — an `https` origin can never be
  same-origin with the `http` request the backend actually receives, so it is
  accepted only for a declared name.

`CHAOS_WEB_PUBLIC_ORIGIN=https://<name>[:<port>]` is that declaration. It must be
a bare `https` origin — no path, query, fragment or credentials — and it requires
`CHAOS_WEB_TOKEN`: a server that answers a public name is never anonymous. The
proxy must then set `X-Forwarded-Proto: https`; without that claim the `https`
origin is refused, so a plaintext hop cannot present itself as the TLS deployment.
`X-Forwarded-For` is not used for any authorization decision.

A proxy that "fixes" the 401 by rewriting `Host` to the upstream
(`proxy_set_header Host $proxy_host;`) makes plain requests work while the
WebSocket keeps failing, because a WebSocket handshake always carries an `Origin`.
Every refusal says which rule fired in its JSON body — `host_not_allowed`,
`origin_not_allowed`, `origin_requires_forwarded_proto` or `credential_required` —
so the operator edits the thing that is actually wrong.

```sh
scripts/web-deployment-in-docker.sh 2>&1 | tee "web-deployment-$(date +%F).log"
```

That lab runs the built binary behind stock nginx with a lab-issued certificate
chain and checks the handshake, the credential, the declared-name rules, a real
`wss:` session, a credential rotation, Safe Web Mode through the proxy, and that
the backend is unreachable from anywhere but the proxy. It is a deployment-shape
check on Linux; it is not a certificate-authority, CDN or multi-tenant review.

## Previewing a dev server through the Web host

`CHAOS_WEB_PREVIEW_PORTS=3000,5173` maps each named port to `http://127.0.0.1:<port>`
and serves it under `/preview/<port>/`, so a browser that reaches this host by name
can use a dev server that has no authentication of its own without that server
becoming reachable by anyone else. Only what a prefix actually breaks is rewritten:
`Host` and `Origin` become the upstream's own (a request that carried no `Origin` is
given the upstream's, which is what a direct request would have looked like),
`Set-Cookie` is scoped to the prefix with `Domain` dropped, `Location` is put back
under the prefix, and a WebSocket upgrade is bridged with the proxy's own handshake.
The browser's `Sec-WebSocket-Key`, `Sec-WebSocket-Version`, `Connection` and
`Upgrade` belong to the other connection and are not forwarded; its subprotocol offer
and its cookies are. Request bodies are buffered up to 32 MiB rather than capped at
the API's 64 KiB, because uploading to a dev server is a thing dev servers do.

Two things to know before blaming the proxy:

- The app has to build its own URLs under the prefix — Vite's
  `base: '/preview/3000/'`, webpack's `publicPath`. No proxy can rewrite a
  `/main.js` that the app's own HTML already resolved against this host.
  `X-Forwarded-Prefix` is sent on every request so a server-rendered app can do the
  same, and the app's HMR client picks its own socket host and port, so a dev server
  that hard-codes its port produces a socket that bypasses this host.
- A preview is refused unless the request arrived over loopback, even when
  `CHAOS_WEB_PUBLIC_ORIGIN` is set (`preview_loopback_only`). Publishing this host
  and publishing someone's dev server are two different decisions; the second one is
  `CHAOS_WEB_PREVIEW_ALLOW_PUBLIC=1`. From another machine the answer is usually
  `chaos-remote forward`, which puts the remote preview behind this machine's
  loopback instead.

Refusals are JSON with a reason code — `preview_disabled`, `preview_port_not_allowed`,
`preview_loopback_only`, `preview_target_invalid`, `preview_upstream_unreachable`,
`preview_body_too_large`, `preview_upstream_failed`, `preview_upgrade_unsupported` —
and `GET /preview` lists what is enabled. `cargo test -p xai-grok-web` drives the
built binary against a stand-in dev server that refuses a `Host` or `Origin` that is
not its own, and against a raw socket server that reads the proxied handshake
byte-for-byte and answers with the accept key derived from the key it was handed.

## Auto-update tests

`cargo test -p xai-grok-update` runs the whole updater against fake feeds it starts
itself, so it needs no network and no GitHub token. Two things about it surprise people:

- `tests/test_update_feed_e2e.rs` drives the shipped `run_update`, the same entry point
  `chaos update` reaches, with `CHAOS_GH_API_BASE` pointed at a wiremock server and
  `CHAOS_GH_DOWNLOAD_BASE` at a raw HTTP server that can cut a transfer mid-body
  (`tests/common/artifact_server.rs`). The artifacts it installs are `/bin/sh` scripts
  that append to a marker file, so "was this build actually executed" is a fact about a
  file rather than about a log line. Each test calls `assert_feed_is_loopback()` before
  updating: an earlier draft released its mock guard early, cleared the override, and
  installed a real release off github.com. If you touch that file, keep that assertion.
- The tests mutate process environment (`GROK_HOME`, the two feed overrides,
  `CHAOS_REQUIRE_SIG`), so they are `#[serial]`. `tests/common/mod.rs::reset_home()`
  removes `CHAOS_GH_API_BASE` and `CHAOS_GH_DOWNLOAD_BASE` on purpose — a guard dropped
  late would otherwise re-point the next test at a dead server.

`chaos update` verifies a `.sig` sidecar against a public key that is fixed at build
time, so no in-tree test can produce a signature its own build accepts. What the suite
can prove is the refusal half; the accept half has to run against a real published
release, which is what `scripts/verify-release-signature.sh` does.

## Release versioning

One number is the release version, and it lives in
`crates/codegen/xai-grok-pager/npm/chaos/package.json`. `release.yml`'s
`resolve-version` step reads it when the dispatch form left the version blank, so that
file decides the tag, the `GROK_VERSION` stamped into the binaries, and the npm versions
`scripts/ci/stamp-npm-version.mjs` writes.

Everything a user can hold has to carry that same number:

- the six `npm/chaos-<platform>/package.json` files and the meta package's
  `optionalDependencies` pins, because a pin npm cannot resolve makes
  `npm install chaos-code` succeed with no binary inside;
- `xai-grok-pager` and `xai-grok-pager-bin`, because those two crates build the binary
  whose `chaos --version` output the auto-updater compares against a release feed;
- a `## <version>` section in `CHANGELOG.md`, so a tag is never cut for a version this
  repository does not describe.

`scripts/ci/check-version-lockstep.py` checks all of that and runs in CI.

The other thing a release has to get right about its artifacts is not the number but the
name, and it is spread over one more place per installer. `.github/workflows/release.yml`
decides the names in six `copy_one` lines; `install.sh` derives one from a bash `case`,
`install.ps1` from a PowerShell function with an environment-variable fallback,
`install.bat` from three `set` lines, and `chaos update` from `version.rs`'s
`gh_release_asset_name` (whose test reads the workflow file rather than restating it).
`scripts/ci/test-installer-asset-names.py` compares all of them against the `copy_one`
lines, executing `detect_platform` and `Get-AssetName` where the host can run them, and it
runs in CI -- on the `platform tests` legs with `--require`, so a runner without PowerShell
fails instead of quietly checking less. Rename one asset on either side and this is the
check that says so, rather than a Windows user's install returning a 404 that looks like a
missing version.

Four crates deliberately do **not** follow the release version, and the check fails if
this list and that paragraph drift apart: `xai-grok-web` and `xai-grok-desktop` are
versioned by their own bundles, `chaos-engine` is versioned by the protocol it speaks,
and `xai-grok-update` still carries the numbering it inherited from upstream. None of
them publishes an artifact of its own, so nothing a user installs reads their version.
Bumping one of them to match the release version is not a fix and the check will not ask
for it.

With `--published` the same script also asks npm registry what it serves for the seven
names. That half is opt-in because it needs network; run it before a release. It reports a
name whose published versions are merely behind the repository as a note (normal between
releases), and fails on a name npm holds as a security placeholder, which publishing does
not fix — see the npm row under `## Running the installers the way a user does`.

## Checking a published release

`scripts/verify-release-signature.sh` downloads the artifact, the `.sig` sidecar and
`SHA256SUMS` from a published release, recomputes the digest, and runs
`xai_grok_signature::verify_file` — the shipped verifier, in the leaf crate the
updater and `chaos-remote install` both link — over the real bytes.

```sh
scripts/verify-release-signature.sh                 # latest release, host platform
scripts/verify-release-signature.sh --tag v0.4.2    # a specific release
scripts/verify-release-signature.sh --tag v0.4.2 --all   # every artifact in SHA256SUMS
```

It needs `curl` and `gh` (the trusted key is a repository *variable*, so `gh variable
get` reads it in plain text; nothing secret is involved) and it downloads a
hundred-ish megabyte artifact into a temp directory. It also checks two things that make
the first check worth anything: a one-byte corruption of the same artifact must be
refused, and a build given no key must refuse rather than accept.

Because the key is baked in by `option_env!`, `crates/codegen/xai-grok-signature/build.rs`
declares `cargo:rerun-if-env-changed=CHAOS_SIGNING_PUBLIC_KEY`. Without it, rebuilding
after changing that variable relinks nothing and silently keeps whichever key the
previous build embedded. The script's third check is what notices if that directive is
ever removed.

Key generation, the exact secret and variable names, who can sign, and what rotating the
pair does to already-installed binaries is in [docs/release-signing.md](docs/release-signing.md).

## Running the installers the way a user does

The release path has three halves, and the last two are only checkable by installing.

`scripts/install-sh-in-docker.sh` copies the working-tree `install.sh` into a stock
`debian:bookworm-slim` container whose only packages are `curl`, `python3` and
`python3-cryptography` — no cargo, no repo, no prior install — and runs it against the
real published release. It asserts the parts that are easy to fake: that the checksum and
signature checks both reported OK rather than silently skipped, that `bin/chaos` is a
*relative* symlink (so it survives a bind mount that remaps `$HOME`), that the stored
artifact uses the `linux-x86_64` name `chaos update` expects rather than the
`chaos-linux-x64` asset name, and that a second run is a no-op. Two controls keep the
signature line honest: a valid-but-foreign key must refuse and leave the installed
artifact byte-identical, and a present-but-blank key must refuse *before* the download
starts.

A feed that only ever serves correct artifacts cannot say what the checks do when the
bytes are wrong, and a refusal you cannot trigger looks exactly like a check that was
skipped. `scripts/install-integrity-in-docker.sh` is the negative half, and it needs
neither the network nor the signing key. It builds its own release -- an artifact, its
`SHA256SUMS` row, and an Ed25519 `.sig` over the artifact bytes -- and serves it through the
ghproxy-style mirror path `install.sh` already supports
(`${CHAOS_GITHUB_MIRROR}/https://github.com/...`), so the code under test is the real
download path with only its origin replaced. The container runs with `--network none`:
loopback works, DNS resolves nothing, and the run asserts that `github.com` is unreachable
before it installs anything. Scenarios: a tampered artifact; `SHA256SUMS` *recomputed* to
match the tampered bytes so that only the signature stands in the way; a missing sidecar;
a valid key that is not ours; a blank key (which must refuse with zero requests logged);
a manifest with no row for this asset; a manifest served as a 200-with-HTML error page; an
empty download. Every refusal is checked twice -- for the reason and for `bin/chaos` being
absent -- and the two documented escape hatches are measured rather than trusted: skipping
the checksum alone still leaves the signature refusing the tampered artifact, while
skipping both installs it, which is what "you are then trusting the download" means. The
fixture, the mirror and the request-log assertion live in `scripts/ci/release-integrity-{fixture,serve,request-log}.py`
so that the Windows installer below is offered the *same* release rather than a
hand-written lookalike of it.

`scripts/install-integrity-powershell.sh` runs that other installer, `install.ps1`, for
real: pwsh on Linux, no Windows and no docker, against the shared fixture. It re-execs
itself inside `unshare -rn`, so the run has no route but loopback -- a trap worth knowing
about, because a fresh namespace has loopback *down*, and binding `127.0.0.1` succeeds
while every connect is refused. It covers the download-and-verify path the same way the
shell lab does: install, then seven refusals each checked for the reason and for nothing
landing in `~/.chaos/bin`, then the two hatches. It does not cover the three things a real
Windows box adds, and says so in its own header: which asset name
`[RuntimeInformation]::OSArchitecture` would ask for, running a PE binary, and the
registry `PATH` write (every run passes `-NoPath`). So the honest remaining claim is
narrow: the Windows installer is now measured to put the *right bytes* on disk and to
refuse the wrong ones, and still unmeasured on whether Windows will execute them or put
them on `PATH`.

Building it found two things. `install.ps1` refuses any artifact under 1 MiB *before*
hashing it; `install.sh` had no such floor and would happily hash a truncated body and
report whatever the checksum said; `install.bat` used the same 1 MiB number only to decide
whether to sniff for HTML, so a short non-HTML body fell through to `certutil` and came
back looking like a checksum mismatch. All three now refuse anything under 1 MiB up front,
and a check in the shell lab reads all three files and fails if those numbers drift apart.
And both script installers had
the same reporting defect, found from opposite sides: when every candidate fails, only the
*last* candidate's reason was shown, so a mirror that answers 200 with an HTML error page
-- or a fixture that answers 404 -- got blamed on whichever public mirror's DNS failed
last. Both now print up to four distinct reasons as `why:` lines. Deleting the loop from
`install.ps1` makes exactly three of the 30 PowerShell checks fail, which is the evidence
that those checks test something.

Neither lab used to run anywhere but the machine that wrote it. The `installer integrity
labs` job in CI runs both on every push -- no release, no signing key, and no network
needed at run time -- and asserts `pwsh --version` and `unshare -rn true` in its first
step, so a runner image that stops shipping one says so immediately instead of three
minutes into the docker step. Neither lab has a skip path: a missing `pwsh`, a missing
crypto binding, or a kernel that will not create a network namespace exits 2 with a named
reason, so the job cannot go green by measuring nothing.

`scripts/ci/check-powershell-syntax.py` is the cheap predecessor of that lab and still
runs on the `platform tests` legs, where a parse error would otherwise go unnoticed: `install.ps1`
once carried a stray closing brace for a stretch of history, so the documented
`irm .../install.ps1 | iex` died before downloading anything and nothing noticed, because
no Linux job executes it and those legs only build and test the Rust workspace.
It parses every tracked `*.ps1` with a real PowerShell and the legs run it with
`--require` so it cannot pass by finding no PowerShell.

`scripts/npm-install-in-docker.sh` does the same for `npm install -g chaos-code`, in a
stock `node` image, and asserts the container's own registry is
`https://registry.npmjs.org/` — this host's npm points at a mirror, so a host-side run
would have tested the mirror. Because npm filters optional dependencies by platform,
`npm install --os=win32 --cpu=x64` asks the Windows question from Linux: the two Windows
platform packages are pinned to a version that has never been published under those names,
npm skips what it cannot resolve, and the install reports success with no binary. The
script fails on exactly that and says which pins are missing.

Both scripts download a hundred-ish megabyte artifact; `install-sh-in-docker.sh
--skip-wrong-key` trades the foreign-key control for one fewer download.
`install-integrity-in-docker.sh` downloads nothing at all -- its artifact is a shell script
-- so it is the one to run when there is no network or no access to the release feed.

### Why the installers embed the signing key

`install.sh`, `install.ps1` and `install.bat` verify the signature and fail closed when
they cannot, and they used to take the key *only* from `CHAOS_SIGNING_PUBLIC_KEY` — which
no documented install command sets, so `curl -fsSL .../install.sh | bash` could not
complete an install on any machine. Each installer now ships the public key as
`DEFAULT_SIGNING_PUBLIC_KEY` / `$DefaultSigningPublicKey`, overridable by that environment
variable for anyone signing their own releases. Setting the variable to an empty string is
still an error, which keeps the fail-closed branch reachable and testable.

`scripts/ci/test-installer-signature-policy.py` checks that all three installers embed the
same 32-byte key and that the key plus the crypto prerequisites are resolved *before* the
download, since the artifact is 150 MB+. `install-sh-in-docker.sh` additionally checks the
embedded value against the `CHAOS_SIGNING_PUBLIC_KEY` repository variable.

## Upstream reconnaissance

`scripts/upstream-recon.sh` records how far the ported `SOURCE_REV` has fallen
behind `xai-org/grok-build` into `sync/recon/`. It is read-only: it queries
`git ls-remote` and the GitHub compare API, writes nothing outside `sync/recon/`,
and exits non-zero rather than writing a record when the network is unavailable.
Recording a gap is not a port — each upstream change still needs the curated-port
review in `sync/` before it touches ported source.

A record is named `<UTC date>-<upstream tip>.md`, the date being UTC rather than
local time. Identical output is a no-op. A record is never overwritten: when the
record for the current UTC day and tip already exists and the new comparison
differs, the script writes `<UTC date>-<tip>-2.md` (then `-3`, …) beside it instead
of replacing it, so a record that someone has annotated cannot be silently reduced
to the generated table. Read the newest record before deciding whether an upstream
change is worth porting.

## Security reports

Please report security issues through the process described in
[`SECURITY.md`](SECURITY.md). Do not open a public issue for vulnerabilities.

## Licensing of this source

By downloading or using this source, you agree that your use is governed by
the Apache License, Version 2.0. No contributor license agreement is offered
because external contributions are not accepted.
