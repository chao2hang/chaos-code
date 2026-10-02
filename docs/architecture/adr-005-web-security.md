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
