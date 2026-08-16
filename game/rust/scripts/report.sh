#!/usr/bin/env bash
# Calliope diagnostics — build the native harness, write text reports, summarize.
#
#   scripts/report.sh          full suite  (~3 min): 3 seeds, 150y civ, 5-seed sweep
#   scripts/report.sh quick    tuning loop (~1 min): 1 seed, 60y, 3-seed sweep
#
# Reports land in game/reports/. Read SUMMARY.txt first — it collects every
# [WARN] and [FAIL] across all reports; drill into the named report for the
# full tables behind a finding.
set -euo pipefail
cd "$(dirname "$0")/.."

MODE="${1:-full}"
OUT="${2:-../reports}"
mkdir -p "$OUT"

echo "== building diagnose (release) =="
if command -v cargo >/dev/null 2>&1; then
  cargo build --release --bin diagnose --features alloc-count --quiet
else
  nix shell nixpkgs#rustc nixpkgs#cargo -c cargo build --release --bin diagnose --features alloc-count --quiet
fi
BIN="./target/release/diagnose"

SIZE=512
if [ "$MODE" = "quick" ]; then
  SEEDS=(12345)
  CIV_YEARS=60
  ECO_YEARS=40
  TELL_YEARS=80
  SWEEP="512 60 12345 777 31337"
  DET_MONTHS=60
  PROPS="512 40 12345"
  ERA="256 40 8 12345"
  PATINA="512 200 12345"
else
  SEEDS=(12345 777 90210)
  CIV_YEARS=150
  ECO_YEARS=100
  TELL_YEARS=150
  SWEEP="512 100 12345 777 31337 90210 555"
  DET_MONTHS=120
  PROPS="512 60 12345 777 90210"
  ERA="256 60 16 12345"
  PATINA="512 300 12345 777 90210"
fi

run() { # run <outfile> <diagnose args...>
  local f="$OUT/$1"; shift
  echo "-- diagnose $* -> $(basename "$f")"
  "$BIN" "$@" > "$f"
}

for s in "${SEEDS[@]}"; do
  run "terrain-$s.txt"   terrain   "$s" "$SIZE"
  run "climate-$s.txt"   climate   "$s" "$SIZE"
  run "hydro-$s.txt"     hydro     "$s" "$SIZE"
  run "resources-$s.txt" resources "$s" "$SIZE"
done
run "civ-${SEEDS[0]}.txt" civ "${SEEDS[0]}" "$SIZE" "$CIV_YEARS"
if [ "${#SEEDS[@]}" -gt 1 ]; then
  run "civ-${SEEDS[1]}.txt" civ "${SEEDS[1]}" "$SIZE" "$CIV_YEARS"
fi
run "economy-${SEEDS[0]}.txt" economy "${SEEDS[0]}" "$SIZE" "$ECO_YEARS"
run "telling-${SEEDS[0]}.txt" telling "${SEEDS[0]}" "$SIZE" "$TELL_YEARS"
run "determinism.txt" determinism "${SEEDS[0]}" "$SIZE" "$DET_MONTHS"
run "bench.txt" bench
run "sweep.txt" sweep $SWEEP
run "properties.txt" properties $PROPS
run "era.txt" era $ERA
run "patina.txt" patina $PATINA

# ---- summary --------------------------------------------------------------
SUM="$OUT/SUMMARY.txt"
{
  echo "CALLIOPE DIAGNOSTIC SUMMARY"
  echo "mode: $MODE · size: $SIZE · generated: $(date -u '+%Y-%m-%d %H:%M UTC')"
  echo
  total_p=0; total_w=0; total_f=0
  for f in "$OUT"/*.txt; do
    b="$(basename "$f")"
    [ "$b" = "SUMMARY.txt" ] && continue
    p=$(grep -c '^\[PASS\]' "$f" || true)
    w=$(grep -c '^\[WARN\]' "$f" || true)
    fl=$(grep -c '^\[FAIL\]' "$f" || true)
    total_p=$((total_p+p)); total_w=$((total_w+w)); total_f=$((total_f+fl))
  done
  echo "totals: $total_p pass · $total_w warn · $total_f fail"
  echo
  for f in "$OUT"/*.txt; do
    b="$(basename "$f")"
    [ "$b" = "SUMMARY.txt" ] && continue
    echo "-- $b"
    grep -E '^\[(WARN|FAIL)\]' "$f" || echo "   all checks pass"
    echo
  done
} > "$SUM"

echo
echo "== reports written to $OUT =="
grep -m1 "totals:" "$SUM"
echo "read SUMMARY.txt first, then the named reports for the tables behind a finding."
