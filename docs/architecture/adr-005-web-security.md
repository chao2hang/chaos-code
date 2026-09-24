# ADR-005: local Web security baseline

- Status: accepted for M0 local mode
- Date: 2026-09-24

The Web host binds to `127.0.0.1` by default. Health is public for process
supervision; session, handshake, HTTP create, and WebSocket routes require an
`Authorization: Bearer` token when `CHAOS_WEB_TOKEN` is configured. Token
comparison is constant-time and tokens are not accepted from query strings.
Origins are limited to the local Vite origins, request bodies are capped at
64 KiB, and JSON responses include CSP and `nosniff` headers.

Non-loopback binding, public deployment, user/token rotation, and full CSRF
protection remain release gates. `CHAOS_SAFE_WEB_MODE` is now enforced in the
WebSocket backend: mutation messages are rejected before engine dispatch, so it
is not a UI-only feature flag.
The WebSocket uses the same authorization and Origin checks as HTTP routes. The
React client derives `ws:`/`wss:` from the page scheme and preserves an explicit
port and base path; this URL selection does not configure TLS termination or a
reverse proxy. Public HTTPS deployments still need a separately reviewed host
and proxy configuration.
