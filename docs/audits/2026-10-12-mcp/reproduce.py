#!/usr/bin/env python3
"""Run MCP audit characterization tests on a temporary copy, without profile data.

Passing tests demonstrate the audited behavior, including defects. They are not
acceptance tests for a corrected implementation. Run from any directory with
`python3 docs/audits/2026-10-12-mcp/reproduce.py`.
"""

from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    audit = Path(__file__).resolve().parent
    root = audit.parents[2]
    with tempfile.TemporaryDirectory(prefix="moony-mcp-audit-") as temporary:
        scratch = Path(temporary)
        shutil.copytree(
            root / "src-tauri", scratch / "src-tauri",
            ignore=shutil.ignore_patterns("target", ".DS_Store"),
        )
        shutil.copytree(root / "shared", scratch / "shared")
        if (root / "dist").exists():
            (scratch / "dist").symlink_to(root / "dist", target_is_directory=True)
        modules = scratch / "src-tauri/src/services/mcp"
        bank = modules / "bank_accounts.rs"
        content = bank.read_text()
        end = content.rfind("}")
        bank.write_text(content[:end] + (audit / "proofs/bank.rs").read_text() + content[end:])
        for destination, proof in [
            ("investments.rs", "stocks.rs"),
            ("crypto.rs", "crypto.rs"),
            ("mod.rs", "assets.rs"),
        ]:
            with (modules / destination).open("a") as stream:
                stream.write("\n" + (audit / "proofs" / proof).read_text())
        # A separate target directory avoids confusing Cargo's fingerprints
        # between the temporary package and the checkout's package.
        command = [
            "cargo", "test", "--offline", "--lib", "--manifest-path",
            str(scratch / "src-tauri/Cargo.toml"), "--target-dir", str(scratch / "target"),
        ]
        for selector in ["audit_mcp_", "audit_securities_proofs", "readonly_assets_audit"]:
            subprocess.run(command + [selector], check=True, cwd=scratch / "src-tauri")


if __name__ == "__main__":
    main()
