# AI assistant (MCP server)

Moony can serve your data to an AI assistant such as Claude over the Model Context Protocol
(MCP). It is **off by default**, runs only on your own computer, and needs the app to be
unlocked. For what leaves your computer when you use it, see
[PRIVACY.md](../PRIVACY.md#the-optional-mcp-server-ai-assistant).

**Stability.** Tool names and result shapes changed incompatibly in 0.9.1 and 0.10.0 (see
[CHANGELOG.md](../CHANGELOG.md)) and have been stable since. If a later release changes them
incompatibly, the changelog says so.

## What it offers

- **Built-in server.** Streamable HTTP at `http://127.0.0.1:41414/mcp` (the port is
  configurable). No extra software to install.
- **Token authentication.** Every request must carry a bearer token that is generated once and
  stored inside the encrypted database. Anyone with the token can read and modify that data:
  treat it like a password.
- **44 tools: reads and writes.** Read tools cover the portfolio, investments, crypto, banking,
  insurance, loans, real estate, budgets, cashflow, exchange rates, analytics and the stock
  watchlist. Write tools create records (accounts, bonds, loans, properties, other assets,
  insurance policies), bulk-import parsed bank statements and stock, crypto and other-asset
  transactions, and change the watchlist (follow a stock, set a target price, update or append research notes).
  Category suggestions arrive as *pending* suggestions that you confirm in the app.
- **Bank import history.** Each validated nonempty MCP bank import call creates a record in the
  account's import history, just like a CSV upload. Pass the original statement's `fileName` when
  known; otherwise the record is named `MCP`. A statement imported in several calls (about 100
  rows each) produces one record per call. Deleting it removes only its newly inserted
  transactions; pending transactions updated from earlier imports retain their original
  ownership. Older unbatched transactions are never assigned automatically;
  `bank_transactions_assign_import` groups the explicitly listed rows of one earlier MCP import
  call (after your confirmation; same account, written within one minute, never CSV uploads or
  rows already in an import) into a record named `MCP`, so it can be deleted in one step.
- **Amounts in your main currency.** Aggregates are reported in your main currency (the
  `mainCurrency` field), as decimal strings at the currency's own precision; an amount that cannot
  be converted is `null` and its currency is listed in `missingCurrencies`. `exchange_rates_list`
  gives the rates against an explicit base (the euro).
- **Runs while unlocked.** Locking the app closes the port; unlocking reopens it with the same
  address and token, so client configs keep working.
- **Your data goes to the AI client.** Whatever the client reads is processed by it, usually in
  its provider's cloud. Enable the server only for clients you trust.

## Setup

1. Open Moony → **Settings → Integrations** → enable **AI Assistant (MCP Server)**.
2. Copy the ready-made config for your client from Settings:
   - **Claude Code:**
     `claude mcp add --transport http moony http://127.0.0.1:41414/mcp --header "Authorization: Bearer <token>"`
   - **Claude Desktop:** via the `mcp-remote` bridge (config shown in Settings; requires Node.js).
   - **Other clients:** HTTP transport (`"type": "http"`) with an `Authorization` header.

Moony must be **running and unlocked** with the MCP server enabled for the integration to work.
