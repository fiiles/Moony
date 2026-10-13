# Getting help

Moony is a free project maintained by one person. Answers are best-effort and there is no
guaranteed response time.

## Where to ask

| You want to | Go to |
| --- | --- |
| Ask how to do something, talk through an idea, or ask about a commercial license | [GitHub Discussions](https://github.com/fiiles/Moony/discussions) |
| Report a bug | [A new issue](https://github.com/fiiles/Moony/issues/new/choose) with the **Bug report** form |
| Suggest a feature | The **Feature request** form on the same page (for a larger idea, start in Discussions) |
| Report a vulnerability | [SECURITY.md](./SECURITY.md). Do not open a public issue, discussion or pull request |
| Correct a translation | [CONTRIBUTING.md](./CONTRIBUTING.md#translations) |

Before you ask, check the [README](./README.md), [PRIVACY.md](./PRIVACY.md) and
[CHANGELOG.md](./CHANGELOG.md) (the problem may be fixed in a newer version), and the existing
issues and discussions.

## What to include in a bug report

- The Moony version: open the account menu at the bottom of the sidebar and choose **About**.
- Your operating system and version, the CPU type (Apple Silicon or Intel on a Mac, the
  distribution on Linux), and whether you installed from a release or built from source.
- Whether you upgraded from an earlier version of Moony.
- The steps to reproduce the problem, what you expected and what happened, with the exact error
  message.
- The relevant lines of the log: in the About dialog, click **Open logs**. Logs can contain ticker
  symbols and file paths, so read them before you post them.
- For an import problem: the header row of the file only, with any personal values replaced.

## Never post

Balances, account numbers, statements, names, documents, your password, your recovery key or the
MCP token, and screenshots that show any of them. Reproduce the problem with made-up values or
the sample profile ([CONTRIBUTING.md](./CONTRIBUTING.md#demo-data)).

## If you lost your password

The recovery key resets a forgotten password and works together with the key files in the data
folder. If you have lost both the password and the recovery key, nobody, including the
maintainer, can open your data. See [PRIVACY.md](./PRIVACY.md#backups-and-the-recovery-key).
