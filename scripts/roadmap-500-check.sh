#!/usr/bin/env bash
# The Five Hundred spec gate — prints FIVE HUNDRED SPECCED and exits 0
# only when docs/ROADMAP-500.md carries every phase M16..M515 exactly
# once, in ascending order, with a properly worded spec on each line
# and no drafting stubs anywhere.
#
# A phase line is `- M<n> <spec>`. A spec is "properly worded" when it
# has at least 5 words and 30 characters — one-line sketches, but real
# ones. "Don't stop until the script lets you."
set -euo pipefail
cd "$(dirname "$0")/.."

ROADMAP="docs/ROADMAP-500.md"
[ -f "$ROADMAP" ] || { echo "missing $ROADMAP" >&2; exit 2; }

fail=0
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# 1. No drafting stubs may remain anywhere in the file.
if grep -nEi 'to be drafted|TBD|TODO|FIXME|placeholder' "$ROADMAP" > "$tmp/stubs" 2>/dev/null && [ -s "$tmp/stubs" ]; then
  echo "STUB MARKERS remain:"
  cat "$tmp/stubs"
  fail=1
fi

# 2. Phase numbering: every M16..M515 exactly once, ascending order.
grep -E '^- M[0-9]+ ' "$ROADMAP" | sed -E 's/^- M([0-9]+) .*/\1/' > "$tmp/got"
seq 16 515 > "$tmp/want"

got_count=$(wc -l < "$tmp/got" | tr -d ' ')
if ! cmp -s "$tmp/want" "$tmp/got"; then
  echo "PHASE NUMBERING BROKEN — want M16..M515 in order, got $got_count phase lines."
  echo "First divergences:"
  diff "$tmp/want" "$tmp/got" | head -30
  fail=1
fi

# 3. Spec quality: every phase line carries >= 5 words and >= 30 chars.
awk '
  /^- M[0-9]+ / {
    spec = $0
    sub(/^- M[0-9]+ /, "", spec)
    n = split(spec, w, /[[:space:]]+/)
    if (length(spec) < 30 || n < 5) {
      printf "THIN SPEC  %s\n", $0
      thin++
    }
  }
  END { if (thin > 0) exit 1 }
' "$ROADMAP" > "$tmp/thin" || true
if [ -s "$tmp/thin" ]; then
  echo "THIN SPECS (need >= 5 words and >= 30 chars):"
  cat "$tmp/thin"
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo
  echo "FIVE HUNDRED INCOMPLETE — the gate holds."
  exit 1
fi

echo "FIVE HUNDRED SPECCED — $got_count/500 phases M16..M515: unique, ordered, worded, no stubs."
