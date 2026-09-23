# ADR-005: local Web security baseline

- Status: accepted for M0 local mode
- Date: 2026-09-24

The Web host binds to `127.0.0.1` by default. Health is public for process
supervision; session, handshake, HTTP create, and WebSocket routes require an
`Authorization: Bearer` token when `CHAOS_WEB_TOKEN` is configured. Token
comparison is constant-time and tokens are not accepted from query strings.
Origins are limited to the local Vite origins, request bodies are capped at
64 KiB, and JSON responses include CSP and `nosniff` headers.

Non-loopback binding, public deployment, user/token rotation, CSRF protections,
and Safe Web Mode are not yet exposed by this host and remain release gates.
The WebSocket uses the same authorization and Origin checks as HTTP routes.
