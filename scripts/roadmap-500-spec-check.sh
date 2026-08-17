#!/usr/bin/env bash
# The Five Hundred full-spec gate — prints FIVE HUNDRED WRITTEN and
# exits 0 only when docs/roadmap-500/ carries a four-field spec block
# (Intent / Build / Touches / Gate) for every phase M16..M515, exactly
# once, in ascending order across the era files, with no drafting
# stubs and no thin fields. Companion to scripts/roadmap-500-check.sh,
# which gates the one-line sketches in docs/ROADMAP-500.md.
set -euo pipefail
cd "$(dirname "$0")/.."

SPEC_DIR="docs/roadmap-500"
[ -d "$SPEC_DIR" ] || { echo "missing $SPEC_DIR" >&2; exit 2; }

shopt -s nullglob
files=("$SPEC_DIR"/0*-era-*.md)
[ "${#files[@]}" -gt 0 ] || { echo "no era spec files in $SPEC_DIR" >&2; exit 2; }

fail=0
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# 1. No drafting stubs may remain in any era file.
if grep -nEi 'to be drafted|TBD|TODO|FIXME|placeholder' "${files[@]}" > "$tmp/stubs" 2>/dev/null && [ -s "$tmp/stubs" ]; then
  echo "STUB MARKERS remain:"
  cat "$tmp/stubs"
  fail=1
fi

# 2. Phase headings: every M16..M515 exactly once, ascending across
#    the era files in filename order.
cat "${files[@]}" | grep -E '^### M[0-9]+ — ' | sed -E 's/^### M([0-9]+) .*/\1/' > "$tmp/got"
seq 16 515 > "$tmp/want"

got_count=$(wc -l < "$tmp/got" | tr -d ' ')
if ! cmp -s "$tmp/want" "$tmp/got"; then
  echo "PHASE HEADINGS BROKEN — want M16..M515 in order, got $got_count spec blocks."
  echo "First divergences:"
  diff "$tmp/want" "$tmp/got" | head -30
  fail=1
fi

# 3. Titles: every heading carries a real title after the em dash.
if grep -hE '^### M[0-9]+ — ' "${files[@]}" | grep -vE '^### M[0-9]+ — .{6,}$' > "$tmp/titles" && [ -s "$tmp/titles" ]; then
  echo "THIN TITLES (need >= 6 chars after the dash):"
  cat "$tmp/titles"
  fail=1
fi

# 4. Fields: every block carries all four fields, each substantive.
#    Minimums are full-line lengths including the field label.
awk '
  function flush() {
    if (cur == "") return
    if (!intent || !build || !touch || !gate) {
      printf "MISSING FIELDS  %s  (intent=%d build=%d touches=%d gate=%d)\n", cur, intent, build, touch, gate
      bad++
    } else if (intent < 50 || build < 80 || touch < 30 || gate < 50) {
      printf "THIN FIELDS  %s  (intent=%d build=%d touches=%d gate=%d)\n", cur, intent, build, touch, gate
      bad++
    }
  }
  /^### M[0-9]+ — /      { flush(); cur = $0; intent = build = touch = gate = 0; next }
  /^- \*\*Intent:\*\* /  { intent = length($0) }
  /^- \*\*Build:\*\* /   { build  = length($0) }
  /^- \*\*Touches:\*\* / { touch  = length($0) }
  /^- \*\*Gate:\*\* /    { gate   = length($0) }
  END { flush(); if (bad > 0) exit 1 }
' "${files[@]}" > "$tmp/fields" || true
if [ -s "$tmp/fields" ]; then
  echo "FIELD FAILURES (Intent >= 50, Build >= 80, Touches >= 30, Gate >= 50 chars):"
  head -40 "$tmp/fields"
  n=$(wc -l < "$tmp/fields" | tr -d ' ')
  [ "$n" -gt 40 ] && echo "... and $((n - 40)) more."
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo
  echo "FIVE HUNDRED UNWRITTEN — the gate holds."
  exit 1
fi

echo "FIVE HUNDRED WRITTEN — $got_count/500 four-field specs M16..M515: unique, ordered, substantive, no stubs."
