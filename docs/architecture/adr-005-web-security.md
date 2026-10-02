# ADR-005: local Web security baseline

- Status: accepted for M0 local mode
- Date: 2026-09-24

The Web host binds to `127.0.0.1` by default. Protected routes reject requests without a syntactically valid loopback `Host`
authority; userinfo and malformed/out-of-range ports are rejected. Health is public for process
supervision; session, handshake, HTTP create, and WebSocket routes require an
`Authorization: Bearer` token when `CHAOS_WEB_TOKEN` is configured. Token
comparison is constant-time and tokens are not accepted from query strings.
Origins are limited to the local Vite origins, request bodies are capped at
64 KiB, and every JSON response, including public `/health`, includes CSP and
`nosniff` headers.

Non-loopback binding, public deployment, user/token rotation, and full CSRF
protection remain release gates. `CHAOS_SAFE_WEB_MODE` is now enforced in the
WebSocket backend: mutation messages, including persistent settings updates, are
rejected before engine dispatch, so it is not a UI-only feature flag. A real
WebSocket regression verifies the settings remain unchanged in Safe Web Mode.
The WebSocket uses the same authorization and Origin checks as HTTP routes. The
React client derives `ws:`/`wss:` from the page scheme and preserves an explicit
port and base path; this URL selection does not configure TLS termination or a
reverse proxy. The Vite development server proxies the same `/health`, `/api`, and
`/ws` routes to the loopback Web host; these routes remain subject to its ordinary
Host, Origin, token, and Safe Web Mode checks. Public HTTPS deployments still need
separately reviewed host and proxy configuration.

## Proxy-fronted TLS (2026-10-02)

A deployment that terminates TLS in front of the loopback host used to be
impossible rather than merely unreviewed: `Host` was refused unless it was a
loopback authority, and an `https` `Origin` was refused unconditionally, because
same-origin could only ever be `http` against the request's own `Host`. The React
client had already chosen `wss:` for HTTPS pages, so the two halves disagreed.

`CHAOS_WEB_PUBLIC_ORIGIN` is now the operator's declaration of the origin the
deployment is reached through. It is a bare `https` origin — path, query,
fragment and credentials are rejected, and a malformed value stops startup rather
than being ignored — and it requires `CHAOS_WEB_TOKEN`. With it set, that exact
authority is accepted in `Host`, and an `https` `Origin` matching it is accepted
only when the proxy also sends `X-Forwarded-Proto: https`. Loopback `Host` values
stay valid, since the proxy dials them and supervision checks arrive there;
nothing is accepted when the variable is unset. `X-Forwarded-For` remains unused
for authorization. A refusal names the rule that fired in its 401 body
(`host_not_allowed`, `origin_not_allowed`, `origin_requires_forwarded_proto`,
`credential_required`); the rules fail identically to a user, and only one of
them is fixed in the proxy.

`scripts/web-deployment-in-docker.sh` drives the built binary behind nginx with a
lab certificate chain and covers the handshake, the credential (including
rotation and never reading it from a query string or a log), the declared-name
rules in both directions, a real `wss:` session, Safe Web Mode through the proxy,
and the backend being unreachable from another machine on the same network.
Remaining gates: certificate issuance and revocation, CDN behaviour, rate
limiting, audit-log retention, and review of an operator's actual proxy config.

## Preview proxy (2026-10-02)

`/preview/<port>/` reaches a dev server on this machine's loopback. The decision
worth recording is what stands in front of it: a dev server normally has no
authentication of its own, so the proxy is the only thing between it and whoever can
open a socket to this host.

Three checks run before any dev server is dialed, each refused with its own reason
code. The existing `Host` rule. The existing `Origin` rule. And one added here: a
preview is served over loopback only, even when `CHAOS_WEB_PUBLIC_ORIGIN` declares a
public name (`preview_loopback_only`). Publishing this host and publishing a
project's dev server are two different decisions, so the second needs its own opt-in
(`CHAOS_WEB_PREVIEW_ALLOW_PUBLIC=1`) rather than riding along with the first; from
elsewhere the answer is `chaos-remote forward`, which puts the preview behind the
requester's own loopback.

The host's bearer token is deliberately absent from both directions of this path. It
is not forwarded upstream — an app running on the developer's own laptop has no claim
on the credential that authenticates this server — and it is not required of the
previewed page either, because a browser attaches `Authorization` to no subresource
request: not the script, not the stylesheet, not the HMR socket. A path that demanded
it would refuse the page rather than protect it, and minting a session cookie for
previews would introduce a second credential where the guards above already bind the
request to this machine. The consequence is stated rather than glossed: everything
under `/preview/<port>/` is as unauthenticated as the dev server behind it, which is
why the port allowlist is empty by default and loopback is the default reach.

Cookie scoping is the other decision with a security edge. A dev server's
`Set-Cookie: Path=/`, stored against this host, would be sent back to every other
route on it — including the API. `Path` is rewritten to the preview's own prefix and
`Domain` is dropped, so a previewed app cannot widen the scope of anything on the
host that previews it. A WebSocket upgrade is bridged with the proxy's own handshake
for the same reason in reverse: the browser's `Sec-WebSocket-Key`, version and
`Connection`/`Upgrade` tokens describe the browser-facing connection, and letting them
reach the upstream handshake means the two hops disagree about what was negotiated.

The rewritten `Host` and `Origin` are inserted into the client request rather than
appended, which matters because the WebSocket client writes those five names itself
from the first value it finds and discards the rest. That is an assumption about a
dependency, so a unit test pins it — it makes the request URI and the header map
disagree and asserts the map's value is what goes on the wire. Were the client to
start joining values instead, the test says so in place of this paragraph.

`cargo test -p xai-grok-web` drives the built binary against two stand-ins: an axum
server that refuses a `Host` or `Origin` that is not its own, the way Vite does, and
a raw socket server that reads the proxied upgrade byte-for-byte and answers with the
accept key derived from the key it was actually handed. A rewrite that regresses
surfaces as the app refusing the browser, which is the failure a user would see.
