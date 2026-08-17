#!/usr/bin/env bash
# Calliope diagnostics — build the native harness, write text reports, summarize.
#
#   scripts/report.sh          full suite  (~3 min): 3 seeds, 150y civ, 5-seed sweep
#   scripts/report.sh quick    tuning loop (~1 min): 1 seed, 60y, 3-seed sweep
#
# Reports land in game/reports/. Read SUMMARY.txt first — it collects every
# [WARN] and [FAIL] across all reports; drill into the named report for the
# full tables behind a finding.
#
# Env knobs: BROWSER=0 skips the browser probe section (E10.5/E10.7/E9.10);
# it also skips itself when no dev server answers on :8080.
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
  PERF="512 12345 777"
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
  PERF="512 12345 777 90210"
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
run "perf.txt" perf $PERF
run "sweep.txt" sweep $SWEEP
run "properties.txt" properties $PROPS
run "era.txt" era $ERA
run "patina.txt" patina $PATINA

# ---- wasm size budget (E10.4/E6.4) ----------------------------------------
# The shipped binary is a build artifact, not a source claim — measure the
# real file next to the loader. Skipped (not failed) when no wasm has been
# built in this checkout.
WASM="../web/js/wasm/calliope_bg.wasm"
{
  echo "========================================================================"
  echo " CALLIOPE DIAGNOSTIC · WASM                              shipped binary"
  echo "========================================================================"
  if [ -f "$WASM" ]; then
    BYTES=$(wc -c < "$WASM" | tr -d ' ')
    MIB=$(awk "BEGIN{printf \"%.2f\", $BYTES/1048576}")
    echo " calliope_bg.wasm: $BYTES bytes = $MIB MiB"
    echo
    echo "---- checks ----------------------------------------------------------"
    ROW=$(awk "BEGIN{printf \"%-48s %7s MiB\", \"wasm binary size\", \"$MIB\"}")
    if awk "BEGIN{exit !($MIB <= 3.2)}"; then
      echo "[PASS] $ROW   (sweet ≤3.2 MiB · hard ≤4.0 — E6 lean-binary budget)"
    elif awk "BEGIN{exit !($MIB <= 4.0)}"; then
      echo "[WARN] $ROW   (sweet ≤3.2 MiB · hard ≤4.0 — E6 lean-binary budget)"
    else
      echo "[FAIL] $ROW   (sweet ≤3.2 MiB · hard ≤4.0 — E6 lean-binary budget)"
    fi
    echo "CHECKS: see row above"
  else
    echo " no wasm binary at $WASM — build with scripts/build.sh first (skipped, not failed)"
  fi
} > "$OUT/wasm.txt"
echo "-- wasm size -> wasm.txt"

# ---- module-dependency lint (E11.8) ----------------------------------------
# Leaf modules must not import `world` — the import DAG stays acyclic by
# check, not convention. Allowed to speak of World: the orchestrator itself,
# its impl-extension modules (pack/snapshot/famine/prospecting/explain),
# the system lattice, and the bins. Everything else is a leaf: it may use
# `event` (the wire vocabulary), `state` (the walls) and its sibling leaves.
{
  echo "========================================================================"
  echo " CALLIOPE DIAGNOSTIC · MODULE DAG                       E11.8 lint"
  echo "========================================================================"
  ALLOWED="lib.rs world.rs systems.rs pack.rs snapshot.rs famine.rs prospecting.rs explain.rs"
  bad=""
  for f in src/*.rs; do
    b="$(basename "$f")"
    case " $ALLOWED " in *" $b "*) continue ;; esac
    hits=$(grep -n 'crate::world' "$f" || true)
    if [ -n "$hits" ]; then
      bad="$bad$b:
$hits
"
    fi
  done
  leaves=$(ls src/*.rs | wc -l | tr -d ' ')
  echo " scanned $leaves modules · allowed world-importers: $ALLOWED"
  echo
  echo "---- checks ----------------------------------------------------------"
  if [ -z "$bad" ]; then
    echo "[PASS] leaf modules import event/state, never world    (E11.8: import DAG acyclic by check)"
  else
    echo "[FAIL] leaf modules importing crate::world             (E11.8: break the edge or bless the module)"
    printf '%s\n' "$bad"
  fi
  echo "CHECKS: see row above"
} > "$OUT/moddag.txt"
echo "-- module DAG lint -> moddag.txt"


# ---- browser probes (E10.5 boot · E10.7 long tasks · E9.10 GL drill) ------
# Real Chromium against the running dev server; skips itself when the
# server is down so the native harness stays self-contained.
if [ "${BROWSER:-1}" = "1" ] && command -v python3 >/dev/null 2>&1 \
   && curl -sf -o /dev/null --max-time 2 http://localhost:8080/ 2>/dev/null; then
  echo "-- browser probe -> browser.txt (boot, long tasks, context-loss drill)"
  if ! timeout 420 python3 ../../scripts/browser-probe.py "$OUT/browser.txt"; then
    echo "[FAIL] browser probe crashed or timed out — see console" >> "$OUT/browser.txt"
  fi
else
  echo "-- browser probe skipped (no dev server on :8080 or BROWSER=0)"
fi

# ---- perf history (E10.8) --------------------------------------------------
# One dated row per run, append-only: drift across weeks stays visible even
# while every individual run passes its bands.
HIST="$OUT/bench-history.txt"
[ -f "$HIST" ] || echo "# date · mode · gen512 ms · tick mo/s · pack B/cell · tick payload B · wasm MiB" > "$HIST"
GEN=$(grep -oP '512 generation time\s+\K[0-9]+(?= ms)' "$OUT/bench.txt" | head -1 || echo "?")
RATE=$(grep -oP 'tick rate\s+\K[0-9]+(?= mo/s)' "$OUT/bench.txt" | head -1 || echo "?")
BPC=$(grep -oP 'pack bytes per cell\s+\K[0-9.]+(?= B/cell)' "$OUT/bench.txt" | head -1 || echo "?")
PAYLOAD=$(grep -oP 'median tick payload\s+\K[0-9]+(?= B)' "$OUT/bench.txt" | head -1 || echo "?")
WMIB=$( [ -f "$WASM" ] && awk "BEGIN{printf \"%.2f\", $(wc -c < "$WASM")/1048576}" || echo "-" )
printf '%s · %-5s · gen %sms · tick %s mo/s · pack %s B/cell · payload %s B · wasm %s MiB\n' \
  "$(date -u '+%Y-%m-%d %H:%M')" "$MODE" "$GEN" "$RATE" "$BPC" "$PAYLOAD" "$WMIB" >> "$HIST"

# ---- summary --------------------------------------------------------------
SUM="$OUT/SUMMARY.txt"
{
  echo "CALLIOPE DIAGNOSTIC SUMMARY"
  echo "mode: $MODE · size: $SIZE · generated: $(date -u '+%Y-%m-%d %H:%M UTC')"
  echo
  total_p=0; total_w=0; total_f=0
  for f in "$OUT"/*.txt; do
    b="$(basename "$f")"
    case "$b" in SUMMARY.txt|bench-history.txt) continue ;; esac
    p=$(grep -c '^\[PASS\]' "$f" || true)
    w=$(grep -c '^\[WARN\]' "$f" || true)
    fl=$(grep -c '^\[FAIL\]' "$f" || true)
    total_p=$((total_p+p)); total_w=$((total_w+w)); total_f=$((total_f+fl))
  done
  echo "totals: $total_p pass · $total_w warn · $total_f fail"
  echo
  for f in "$OUT"/*.txt; do
    b="$(basename "$f")"
    case "$b" in SUMMARY.txt|bench-history.txt) continue ;; esac
    echo "-- $b"
    grep -E '^\[(WARN|FAIL)\]' "$f" || echo "   all checks pass"
    echo
  done
} > "$SUM"

echo
echo "== reports written to $OUT =="
grep -m1 "totals:" "$SUM"
echo "read SUMMARY.txt first, then the named reports for the tables behind a finding."