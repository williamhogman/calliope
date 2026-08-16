#!/usr/bin/env bash
# Engine-roadmap stopping criterion — prints ENGINE ROADMAP COMPLETE and
# exits 0 when every milestone item in docs/ROADMAP-ENGINE.md is checked.
#
# Scans ## E1 … ## E11 milestone sections for checkbox items. Unchecked
# `- [ ]` items are open work; the script lists them and exits 1.
# "Ready", "Later / research" and "Rejected" sections are out of scope.
set -euo pipefail
cd "$(dirname "$0")/.."

ROADMAP="docs/ROADMAP-ENGINE.md"
[ -f "$ROADMAP" ] || { echo "missing $ROADMAP" >&2; exit 2; }

open=$(awk '
  /^## E[0-9]/ { in_ms = 1; section = $0; next }
  /^## /       { in_ms = 0 }
  in_ms && /^- \[ \]/ { printf "%s  ::  %s\n", section, $0 }
' "$ROADMAP")

done_count=$(awk '
  /^## E[0-9]/ { in_ms = 1; next }
  /^## /       { in_ms = 0 }
  in_ms && /^- \[x\]/ { n++ }
  END { print n + 0 }
' "$ROADMAP")

if [ -z "$open" ]; then
  echo "ENGINE ROADMAP COMPLETE — $done_count items done, 0 open."
  exit 0
fi

count=$(printf '%s\n' "$open" | wc -l | tr -d ' ')
echo "ENGINE ROADMAP OPEN — $done_count done, $count remaining:"
echo
printf '%s\n' "$open"
exit 1
