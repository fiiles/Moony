# Roadmap

Moony is maintained by one person, so this page lists intentions, not commitments. The items
are in rough order, there are no dates, and the order can change. "Planned" means the
maintainer intends to do it, not that it will be done by any particular release. What has
already shipped is in [CHANGELOG.md](./CHANGELOG.md). 0.10 is the beta for 1.0; see
[Status and compatibility](./README.md#status-and-compatibility) in the README.

## Planned, in rough order, no dates

1. **Signed and notarized installers.** Apple notarization and a Windows code-signing
   certificate, so macOS and Windows stop warning you the first time you open the app. Both need
   paid memberships or certificates that the project does not have yet. Until then the
   [install steps](./README.md#download-and-install) apply.
2. **A downloadable demo profile**, so you can try Moony without building anything. Today you
   generate one with `cargo run --bin seed_demo`, see
   [CONTRIBUTING.md](./CONTRIBUTING.md#demo-data).
3. **More importers.** More brokers and more crypto exchanges, and confirmation of the
   best-effort bank presets listed in the README.
4. **MCP scopes and an audit log.** Limits on what a connected AI assistant may read or change,
   and a record of what it did.
5. **A fallback price provider.** Stock prices come from an unofficial Yahoo Finance interface
   that can break; a second source would keep prices available.
6. **In-app documentation.** Today the README is the user documentation.

## Not before 1.0

These come up often. None of them is planned for 1.0 and none is ruled out for later. If one
of them ever ships, it will be optional, and [PRIVACY.md](./PRIVACY.md) will say exactly what it
sends before the release that adds it.

- **Connecting to your bank** instead of importing statements.
- **A phone app** and **sync between your own devices**.
- **A profile shared by a household.**

## Never

- **Telemetry** of any kind: no analytics, no usage statistics, no crash reports.

## Suggesting something else

Ideas are welcome in [GitHub Discussions](https://github.com/fiiles/Moony/discussions).
Please open a discussion before sending a pull request for a larger change, see
[CONTRIBUTING.md](./CONTRIBUTING.md).
