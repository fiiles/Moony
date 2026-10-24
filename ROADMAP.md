# Roadmap

Moony is maintained by one person, so this page lists intentions, not commitments. There are no dates, the order can change, and "planned" means the maintainer intends to do it, not that it will be done by any particular release. What has already shipped is in [CHANGELOG.md](./CHANGELOG.md).

## Today

Moony is a desktop app for macOS, Windows and Linux. You import bank statements or enter data yourself, everything stays on your computer, and there is no sync. This describes today, not a rule: the longer-term items below would change some of it.

## Next

1. **Budgets and goals, as two separate things.** A *budget* tracks your spending so that you stay within a limit (what the Budgets page does today). A *goal* tracks your progress towards an amount you want to reach, for example savings.
2. **A signed Windows installer.** A Windows code-signing certificate, so Windows stops warning
   you the first time you open the app. It needs a certificate that the project does not have
   yet. Until then the [install steps](./README.md#download-and-install) apply.
3. **A downloadable demo profile**, so you can try Moony without building anything. Today you
   generate one with `cargo run --bin seed_demo`, see
   [CONTRIBUTING.md](./CONTRIBUTING.md#demo-data).
4. **More importers.** More brokers and more crypto exchanges, and confirmation of the
   best-effort bank presets listed in the README.
5. **MCP scopes and an audit log.** Limits on what a connected AI assistant may read or change,
   and a record of what it did.
6. **A fallback price provider.** Stock prices come from an unofficial Yahoo Finance interface
   that can break; a second source would keep prices available.
7. **In-app documentation.** Today the README is the user documentation.

## Longer term

Bigger pieces of work that the maintainer wants to do eventually. None has a date or a finished design, and the order is open.

- **A standalone mobile app.** Moony on a phone that works on its own, with its own encrypted data and no desktop app needed.
- **A bank-data connection.** Fetching balances and transactions automatically through a bank-data API service, as an alternative to importing statements.
- **A sync service** that keeps several of your own devices up to date.

Anything that sends data over the internet would be optional and off until you turn it on, and [PRIVACY.md](./PRIVACY.md) will say exactly what it sends, and to whom, before the release that adds it.

## Never

- **Telemetry** of any kind: no analytics, no usage statistics, no crash reports.

## Suggesting something else

Ideas are welcome in [GitHub Discussions](https://github.com/fiiles/Moony/discussions). Please open a discussion before sending a pull request for a larger change, see [CONTRIBUTING.md](./CONTRIBUTING.md).
