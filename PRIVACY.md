# Privacy

Moony is a local-first app: **there is no account, no cloud and no sync.** Your financial
records stay on your computer, in an encrypted database. The app talks to the internet only
for the few things listed below, and none of them involves your amounts, balances, names or
documents.

This page is written to be checked against the code. If you find a statement here that does
not match what the app does, please open a GitHub issue — that is a bug.

## What stays on your computer

Everything you enter or import — bank accounts and transactions, investments, crypto, bonds,
loans, insurance, real estate, budgets, categories and rules, your profile, settings and the
access token of the optional MCP server — is stored in one SQLite database encrypted with
**SQLCipher (AES-256)**.

How the encryption key is handled:

- A random 256-bit master key encrypts the database. The key is never stored in plain text.
- The master key is stored twice, wrapped by a key derived from **your password** (`key.enc`) and by a
  key derived from your **recovery key** (`recovery.enc`), both with scrypt and a shared
  `salt` file. Moony itself cannot read your data without one of the two.
- While the app is unlocked the key lives in memory only. Locking the app closes the database.

## What is *not* encrypted

Be aware of these, because the "encrypted database" claim does not cover them:

| What | Where | Notes |
| --- | --- | --- |
| Photos and documents you attach (real-estate photos and documents, insurance documents) | `real_estate_photos/`, `real_estate_documents/`, `insurance_documents/` in the data folder | Stored as ordinary files. Encrypting attachments is on the roadmap. Anyone who can read your user profile can open them. |
| Backups you create | The `.zip` you choose in Settings → Data and backups | The database inside is still encrypted and the key files need your password or recovery key, but **the attachments inside the archive are plain files**. |
| "Export all data (JSON)" | The file you choose | A plain-text JSON dump of your data (API keys and the MCP token are left out). Handle it like a bank statement. |
| UI preferences | The operating system's WebView profile (`localStorage`) | Theme and device-level settings such as auto-lock. No financial data. |
| Application logs | The log folder (About → Open logs) | Meant for troubleshooting. They can contain technical details such as ticker symbols and file paths. Read them before attaching them to a public bug report. |

Full-disk encryption (FileVault, BitLocker, LUKS) is the right complement to all of the above.

## What leaves your computer, and when

Moony makes network requests only in these situations. Every request also reveals your IP
address to the service that receives it, as any internet request does.

| Service | What is sent | When | Can you avoid it? |
| --- | --- | --- | --- |
| **Yahoo Finance** (`query1/query2.finance.yahoo.com`) | The ticker symbols of your stock positions and of the stocks you follow in Stock Monitor, search text you type, the symbols and ISINs of the securities in a stock CSV file you import (to find their Yahoo tickers), and the date ranges needed for price history (which can reveal roughly when you started investing). | After unlocking, when you add or import stock transactions (the review step of a stock CSV import looks the file's securities up as it opens), when you open Stock Monitor or a stock position's detail (company data such as sector and P/E, at most once a day per ticker once Yahoo has answered), and when you refresh stale prices. | Yes — Moony works offline; prices can be set manually per position. Without refreshes the prices simply go stale. |
| **CoinGecko** (`api.coingecko.com`) | The CoinGecko IDs of your crypto holdings (that is, the composition of your crypto portfolio), search text, and date ranges for history. If you enter your own optional CoinGecko API key in Settings → Integrations, it is sent as a request header. | After unlocking, when you add a holding, when history is back-filled. | Yes — same as above. |
| **European Central Bank** (`www.ecb.europa.eu`) | Nothing about you; it is a download of the public daily reference rates. | When exchange rates are stale. | Yes — rates then simply go stale. Currencies the ECB does not publish never get an automatic rate (Settings → General → Currency explains this). |
| **Frankfurter** (`api.frankfurter.dev`, serves ECB historical rates) | The date range from your oldest transaction to today and the list of currencies used in your transactions. | When you enter or import transactions in a currency whose history is missing, and on rate refresh. Best effort: failures are ignored. | Yes — without it historical conversions fall back to current rates. |
| **GitHub** (`github.com`, `objects.githubusercontent.com`) | A plain request for `latest.json` of the latest release; the updater adds its user agent (app name and version, operating system, CPU architecture). If you accept an update, the installer is downloaded from GitHub and its signature is verified against the public key built into the app. | Once per app start, after you unlock. | Yes — Settings has a switch for the automatic check. |
| **Your AI client** (MCP, optional) | Whatever the client asks for and the tools return — potentially your full financial data. | Only while the MCP server is enabled, the app is unlocked and a client connected with your token. | Off by default; see below. |

Notes:

- The web view that renders the interface is restricted by a content-security policy to its
  own files: it does not load fonts, scripts, logos or images from any website.
- All data requests go through the Rust backend; the interface itself makes no network calls.

### The optional MCP server (AI assistant)

If you switch on **Settings → Integrations → AI Assistant (MCP Server)**, Moony runs a small
server on `127.0.0.1` only (default port 41414). Every request must carry a bearer token that
is generated once and kept in the encrypted database; anyone holding the token can **read and
change** your data, so treat it like a password. The server runs only while the app is
unlocked and stops when you lock it.

Moony does not send anything to an AI provider itself. But an AI client such as Claude
Desktop or Claude Code that you connect will process whatever it reads — usually in its
provider's cloud, under that provider's terms. Enable the integration only for clients you
trust, and regenerate the token when you stop using one.

## What Moony does not do

- No account, login or registration, and no server operated by the project.
- No telemetry: no crash reporting, no analytics, no tracking SDKs.
- No advertising, no cookies, no third-party fonts, images or logos.
- No remote configuration, no remote code and no way for anyone to read your data remotely.

## Where your data lives

Moony stores its files in the application data folder, whose name comes from the app
identifier `com.filipkral.moony`:

| OS | Data folder |
| --- | --- |
| macOS | `~/Library/Application Support/com.filipkral.moony/` |
| Windows | `%APPDATA%\com.filipkral.moony\` (usually `C:\Users\<you>\AppData\Roaming\…`) |
| Linux | `~/.local/share/com.filipkral.moony/` (or `$XDG_DATA_HOME/com.filipkral.moony/`) |

Inside it:

| Entry | What it is |
| --- | --- |
| `moony.db` | The encrypted database |
| `salt`, `key.enc`, `recovery.enc` | Key material. Useless without your password or recovery key — and your data is unreadable without them |
| `real_estate_photos/`, `real_estate_documents/`, `insurance_documents/` | Your attachments (**not encrypted**) |
| `moony.db.bak-<timestamp>` | A copy of the database that Moony makes just before applying a database migration after an update (a few of the newest copies are kept) |
| `pre-restore-<timestamp>/` | The data you had before a restore from backup, kept so a restore can never destroy anything |

**Settings → Data and backups** shows the exact path on your machine and has an "Open folder"
button. Moony never moves this folder on its own, and uninstalling the app does not delete it
by default.

## Backups and the recovery key

You are the only holder of your data, so you are responsible for keeping copies:

- **Back up regularly** with *Settings → Data and backups → Back up now*. The archive contains
  the encrypted database, the key files and your attachments, and can be restored on another
  computer with *Restore from backup*. Store it somewhere safe (an external drive, your own
  cloud storage). Restoring never deletes your current data; it is moved to a `pre-restore-…`
  folder first.
- **Save your recovery key** when it is shown (the first-run wizard asks you to verify it). It
  is 24 characters long and is the only way to reset a forgotten password. It works **only
  together with the key files** (`salt`, `recovery.enc`) in the data folder, so a backup of the
  whole folder or a Moony backup archive is part of the recovery plan.
- **There is no backdoor.** If you lose both your password and your recovery key, nobody —
  including the author — can decrypt your database.
- **Leave the identifier alone** if you build Moony yourself: the data folder is derived from it,
  and changing it makes the app look for your data somewhere else.

## Deleting your data

*Settings → Account → Delete Account* (you must type `DELETE`) removes the database, the key
files and all attachments from the data folder. It does not touch backups or exports you
created yourself — delete those separately. There is nothing to delete on any server because
Moony has none.

## Questions and changes

Questions about privacy: open a GitHub Discussion or issue. Vulnerabilities: please follow
[SECURITY.md](./SECURITY.md) instead of filing a public issue. This document changes together
with the code; its history is in git, and notable changes are listed in
[CHANGELOG.md](./CHANGELOG.md).
