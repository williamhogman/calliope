#!/usr/bin/env bash
# Roadmap stopping criterion — the run is done when this prints
# ROADMAP COMPLETE and exits 0.
#
# Scans docs/ROADMAP.md milestone sections (## M1 … ## M9) for
# checkbox items. Unchecked `- [ ]` items are open work; the script
# lists them and exits 1. "Later / research" and "Rejected" sections
# are out of scope by design and never counted.
set -euo pipefail
cd "$(dirname "$0")/.."

ROADMAP="docs/ROADMAP.md"
[ -f "$ROADMAP" ] || { echo "missing $ROADMAP" >&2; exit 2; }

# Collect milestone-section lines only (## M<digit> … up to the next ## that
# is not a milestone), then pick out checkbox items.
open=$(awk '
  /^## M[0-9]/ { in_ms = 1; section = $0; next }
  /^## /       { in_ms = 0 }
  in_ms && /^- \[ \]/ { printf "%s  ::  %s\n", section, $0 }
' "$ROADMAP")

done_count=$(awk '
  /^## M[0-9]/ { in_ms = 1; next }
  /^## /       { in_ms = 0 }
  in_ms && /^- \[x\]/ { n++ }
  END { print n + 0 }
' "$ROADMAP")

if [ -z "$open" ]; then
  echo "ROADMAP COMPLETE — $done_count items done, 0 open."
  exit 0
fi

count=$(printf '%s\n' "$open" | wc -l | tr -d ' ')
echo "ROADMAP OPEN — $done_count done, $count remaining:"
echo
printf '%s\n' "$open"
exit 1
