# 0004. Local API token-gated writes

Date: 2026-07-07

## Status

Superseded by [0006](0006-embedded-mcp-server.md)

## Context

Moony runs a local HTTP API (`src-tauri/src/services/local_api.rs`) so
external tools and AI agents (MCP) can work with portfolio data. The README
described this server as read-only, yet it exposes a write endpoint
(`POST /insurance`). The mismatch made an intentional capability look like an
accident — or a vulnerability — and left agents unsure whether adding write
endpoints is allowed.

## Decision

The local API's write surface is governed by these rules:

- **Write endpoints on the local API are an intentional, kept feature.** The
  server exists for agent integration, and useful agents need writes.
- **The protection model is off-by-default plus bearer token:** the server
  only runs when the user enables it in their profile
  (`mcp_server_enabled`); it binds to `127.0.0.1` on an ephemeral port; every
  request must carry the per-session bearer token published in the app data
  dir's `session.json`.
- **The README must describe this honestly** — token-gated read/write access,
  off by default — instead of claiming read-only.

## Consequences

- **Every new write endpoint must be listed in the README's API section** in
  the same change that adds it. An undocumented write endpoint recreates
  exactly the dishonesty this ADR removes.
- Accepted risk: any local process that can read `session.json` while the
  server is enabled can modify financial data. The user's enable toggle is
  the consent boundary.
- The token check and route table live in one file (`local_api.rs`), so
  auditing the exposed surface stays a one-file read.
