# ADR-004: remote topology and current support boundary

- Status: accepted security boundary; remote implementation Deferred outside first stable
- Decision record: [ADR-007](adr-007-first-stable-product-scope.md)
- Date: 2026-10-01

The first supported topology is local Agent plus local workspace. Web and
Desktop transports do not expose remote execution, arbitrary file writes,
terminal sessions, or port forwarding. Remote Agent plus remote tools remains a
future M4 implementation and must use a dedicated authenticated transport,
host-key verification, workspace confinement, cancellation, and upgrade
rollback.

The UI capability declaration must keep remote PTY, detached Agent, container
execution, and public Web deployment as unsupported until those tests and
security controls exist. This decision prevents a local loopback prototype from
being mistaken for a remote control plane.

## Failure model for a session (2026-10-02)

A remote session has two clocks, and the difference between them is the decision.
Opening the transport may be retried: a dial presents nothing, so waiting out a
tunnel that is still being set up costs time and nothing else. A request that has
already been sent may not be retried: its reply is either the answer to it or
nothing at all, because a late reply would be read as the answer to whichever
request comes next. So the client bounds both waits — the handshake at
`DEFAULT_HANDSHAKE_TIMEOUT`, each request at whatever the caller sets — and ends
the session when a reply does not arrive, instead of pretending the stream is
still usable.

There is deliberately no automatic reconnect. A credential opens exactly one
session, so reconnecting means obtaining a new credential from whoever runs the
command, and the transport does not get to decide that on their behalf. A run that
fails still retires the credential it sent, so the next invocation starts from a
token file that has not been left holding a spent credential.

## Local port forwarding (2026-10-02)

`chaos-remote forward --to HOST:PORT` is local forwarding in the `ssh -L` sense: it
puts a loopback listener on the machine that ran the command, and every connection
that arrives there is carried to one service on the *server's* side of the tunnel.
Remote forwarding — `ssh -R`, where the server would open a listener that reaches
back toward the client — is a different direction of trust and is not implemented.
The endpoint type is named (`remote-forward`) so that a configuration asking for it
fails with a message that says so and points at `port-forward`, rather than silently
forwarding in whichever direction happened to be written.

Four decisions make this different from "the session may open any socket":

- **The operator picks the target.** `--allow-forward-to host:port` is a repeatable
  allowlist that defaults to empty, and an empty one can only refuse — so a server
  asked to advertise `port-forward` with no target listed refuses to start, and
  naming a target brings the capability with it, so there is one way to say it. The
  comparison is made against the address the operator typed, not against a name the
  session chose, and it happens when the grant is asked for, so a refusal arrives
  before anything is listening locally.
- **A forward is not a session.** Each connection that arrives on the local port
  dials the server again and presents a *forward ticket* held in its own vault,
  separate from the session-credential one. Such a connection is granted the
  `port-forward` capability and nothing else: it cannot list, read or write the
  workspace it happens to share a port number with. The ticket is bound to one
  `host:port` at issue time; the request cannot name a target, so a ticket cannot be
  pointed somewhere else after the fact.
- **A grant is spent, and it is revoked.** The server answers a request for N
  connections and T seconds with the number and lifetime it will actually agree to,
  capped by `--forward-max-uses` and `--forward-ttl`. When the budget or the TTL runs
  out the client's listener closes — a forward does not linger after its purpose —
  and closing the session that asked for the grant withdraws the ticket, so a
  forward cannot outlive the session that authorised it.
- **The local end is loopback-only,** for the same reason the server's own listener
  is: a forwarded service on a routable interface is a published service, and
  publishing is not what "let me reach it from here" asked for. Port 0 asks the
  operating system for a free port and the chosen one is reported on stderr before
  anything connects; a port already held is refused by name, because binding
  somewhere else would leave whoever was told about the listener talking to a port
  that forwards nothing.

What a forwarded connection is after its handshake is raw bytes, pumped in both
directions until both sides have finished. Ending on the first EOF instead would
reset the connection and can discard bytes the target had already written, which for
a proxy is data loss, so long-lived WebSocket traffic works through the tunnel the
same way plain HTTP does.

`scripts/remote-acceptance-in-docker.sh` runs this against a deployed server: a
`python3 -m http.server` on the remote host is fetched through a forward started
from a second container, the returned digest is compared with the file on that
host's disk, a target outside the allowlist is refused although a listener really is
there, a WebSocket service on the remote host completes its handshake through the
tunnel and answers with a file the developer container cannot read, and a
two-connection grant is seen to close its own listener.

## What an installed artifact has to prove (2026-10-02)

`install` is the one command in this topology that changes what the remote host *runs*,
as opposed to what it reads or writes, so it is checked as such. Three things are known
about an artifact before any file is moved, and each answers a question the previous one
cannot:

1. **Did it arrive whole** — the sha256 the client sent. Necessary, and useless on its
   own: the digest is computed by whoever is sending the bytes.
2. **Who produced it** — an ed25519 signature over those bytes, checked against a key
   this host was told to trust. This is the release key (`CHAOS_SIGNING_PUBLIC_KEY`,
   `--trust-signing-key`), which is why the topology adds no second trust anchor and no
   second ceremony.
3. **Whether this machine could start it at all** — the target the file's own ELF /
   Mach-O / PE header states, compared with the platform the host is running on. An
   intact, correctly signed build for another CPU is still not an upgrade.

The host requires a signature by default, including when it has no key to check one
against: a host that installs whatever it is handed until someone remembers to configure
a key has the same exposure as having no check, and has it invisibly. `--allow-unsigned-artifact`
and `CHAOS_REMOTE_REQUIRE_SIGNATURE=0` are the named opt-outs, for the case they exist
for — an operator deploying a build they produced — and a signature that arrives at such a
host is verified anyway. Either way the host prints its policy at startup, because a policy
nobody can read is a policy that gets misdiagnosed.

Ordering is the part that is easy to get wrong and impossible to notice later: all three
checks run on the staged upload, before the version directory exists and before the
`current` pointer moves. A refusal therefore leaves the previous version selected, the
bytes it ran unchanged, and no half-written artifact to be discovered by the next
operator. The failure that motivated this — an upload whose digest was recomputed over
swapped bytes — passes check 1 by construction, which is why check 2 is not treated as a
nicer error message for a check 1 failure.

What is deliberately *not* claimed: provenance does not restrict what an authorised
session may do. A session holding `tool-execution` can already run the programs its
server allowlisted, and one holding `write` can already change workspace files. What
these checks close is the step no capability implies — these bytes becoming the program
that starts next.
