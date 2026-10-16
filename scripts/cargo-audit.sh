#!/usr/bin/env bash
# Blocking Rust dependency audit, as CI runs it. Run it from the repository root.
#
# Pass 1: vulnerabilities, unsound and unmaintained crates fail; the advisories listed in
# .cargo/audit.toml are the only exceptions.
# Pass 2: yanked crates fail unless .cargo/audit-yanked.txt lists them with a reason, and a
# listed crate that is no longer in the lockfile fails too, so the list never goes stale.
set -euo pipefail

lock=src-tauri/Cargo.lock
allow=.cargo/audit-yanked.txt

cargo audit --file "$lock" --deny warnings --no-yanked

found=$(cargo audit --file "$lock" --no-fetch --json |
  jq -r '.warnings.yanked // [] | .[] | "\(.package.name)@\(.package.version)"' | sort -u)
allowed=$(awk '!/^[[:space:]]*#/ && NF { print $1 }' "$allow" | sort -u)

unexpected=$(comm -23 <(printf '%s\n' "$found") <(printf '%s\n' "$allowed") | sed '/^$/d')
stale=$(comm -13 <(printf '%s\n' "$found") <(printf '%s\n' "$allowed") | sed '/^$/d')

status=0
if [ -n "$unexpected" ]; then
  echo "error: yanked crates in $lock (update them, or list them with a reason in $allow):"
  printf '  %s\n' $unexpected
  status=1
fi
if [ -n "$stale" ]; then
  echo "error: $allow lists crates that are no longer yanked in $lock (delete their lines):"
  printf '  %s\n' $stale
  status=1
fi
if [ "$status" -eq 0 ]; then
  echo "yanked crates: ${found:-none} (all listed in $allow)" | tr '\n' ' '
  echo
fi
exit "$status"
