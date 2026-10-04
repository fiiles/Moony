# 0006. Embedded MCP server over Streamable HTTP

Date: 2026-08-12

## Status

Accepted

Supersedes the discovery/session model of [0004](0004-local-api-token-gated-writes.md);
0004's decision that token-gated writes are an intentional feature stands.

## Context

AI integration required a separately installed Node.js proxy (moony-mcp) that
discovered the app's local REST API through a plaintext `session.json`
(ephemeral port + per-session token). Install friction, a plaintext token on
disk, per-unlock reconfiguration, and a second codebase made this the wrong
shape for a public release. MCP's Streamable HTTP transport lets clients
connect to a URL directly, and the official Rust SDK (rmcp) mounts on the
axum server the app already runs.

## Decision

- Moony itself serves MCP at `http://127.0.0.1:<port>/mcp` (rmcp, Streamable
  HTTP). The REST routes and `session.json` are deleted; the Node proxy is
  retired.
- Fixed port, default 41414, stored in `user_profile.mcp_server_port`,
  configurable in Settings.
- Stable bearer token (UUID v4), generated on first enable, stored in
  `user_profile.mcp_server_token` inside the encrypted database — never
  written to plaintext disk. Shown masked in Settings; regenerable, which
  deliberately invalidates existing client configs.
- Lifecycle unchanged: off by default; runs only while unlocked with
  `mcp_server_enabled`. A bind failure must not fail unlock — it surfaces as
  a status error in Settings.
- Tool methods are synchronous and run their SQLite queries inline on the
  tokio worker — identical behavior to the REST handlers they replaced. The
  DB connection mutex serializes queries anyway and MCP traffic is a single
  local client; revisit only if tools ever do network or multi-second work.
- The write surface stays token-gated and intentional. The honesty rule
  carries over from 0004: **every write tool must be listed in the README**
  in the same change that adds it. `insurance_create` is currently the only
  write tool.

## Consequences

- One external surface: the `/mcp` route in `services/local_api.rs` plus the
  tool bodies in `services/mcp/`. Auditing the surface is still a narrow
  read.
- Client configs survive restarts and unlocks (fixed port + stable token).
- Accepted risk: any process that obtains the token (e.g. from a client's
  own config file) can read and write financial data while the server runs.
  The enable toggle remains the consent boundary; unlike 0004's model, the
  token no longer sits in plaintext in the app's data dir.
- Claude Desktop cannot reach localhost via its connectors; its documented
  path is the `mcp-remote` bridge. Claude Code and other HTTP-native clients
  connect directly.
