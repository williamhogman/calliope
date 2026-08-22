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
  OCEAN="12345 777 31337"
  COMPUTE="512 12345"
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
  OCEAN="12345 777 31337 90210 555"
  COMPUTE="512 12345 777 90210"
fi

run() { # run <outfile> <diagnose args...>
  local f="$OUT/$1"; shift
  echo "-- diagnose $* -> $(basename "$f")"
  "$BIN" "$@" > "$f"
}

for s in "${SEEDS[@]}"; do
  # --explain: the M61 provenance-chain gate rides every terrain run.
  run "terrain-$s.txt"   terrain   "$s" "$SIZE" --explain
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
run "systems.txt" systems "${SEEDS[0]}" "$SIZE" "$CIV_YEARS"
run "earth.txt" earth "$SIZE" "$CIV_YEARS" "${SEEDS[@]}"
run "ocean.txt" ocean "$SIZE" $OCEAN
# M50: the ocean stack answers a perturbation, not just a snapshot.
run "ocean-meta.txt" ocean "$SIZE" $OCEAN --metamorphic
# M63: the cartographic law — palettes, the no-green desert ladder, and
# the pack-v2 lens round-trip — proved at its Rust source.
run "atlas.txt" atlas "$SIZE" "${SEEDS[@]}"

# ---- the compute lane (M67, ADR-0027) --------------------------------------
# The GPU leg must EXECUTE, not be claimed: a second diagnose flavor
# carries the native wgpu backend in its own target dir (same doctrine
# as the assay profile — two flavors never share one rlib path) and is
# pointed at mesa's software Vulkan adapter (lavapipe) so the WGSL
# kernel runs and is byte-compared even on a headless box. If the
# adapter can't be sourced the lane still reports: the CPU twin is the
# law either way, and the report names which legs spoke.
echo "== building diagnose (gpu flavor) =="
if command -v cargo >/dev/null 2>&1; then
  GPU_BUILD=(cargo build --release --bin diagnose --features alloc-count,gpu --quiet)
else
  GPU_BUILD=(nix shell nixpkgs#rustc nixpkgs#cargo -c cargo build --release --bin diagnose --features alloc-count,gpu --quiet)
fi
if CARGO_TARGET_DIR=target/gpu "${GPU_BUILD[@]}"; then
  ICD=""
  VKLIB=""
  if command -v nix >/dev/null 2>&1; then
    for p in $(nix build --no-link --print-out-paths nixpkgs#mesa 2>/dev/null); do
      [ -f "$p/share/vulkan/icd.d/lvp_icd.x86_64.json" ] && ICD="$p/share/vulkan/icd.d/lvp_icd.x86_64.json"
    done
    for p in $(nix build --no-link --print-out-paths nixpkgs#vulkan-loader 2>/dev/null); do
      [ -d "$p/lib" ] && VKLIB="$p/lib"
    done
  fi
  echo "-- diagnose compute $COMPUTE -> compute.txt"
  rm -f "$GOLDEN"
  if [ -n "$ICD" ] && [ -n "$VKLIB" ]; then
    VK_ICD_FILENAMES="$ICD" LD_LIBRARY_PATH="$VKLIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
      ./target/gpu/release/diagnose compute $COMPUTE --golden "$GOLDEN" > "$OUT/compute.txt"
  else
    ./target/gpu/release/diagnose compute $COMPUTE --golden "$GOLDEN" > "$OUT/compute.txt"
  fi
else
  {
    echo "========================================================================"
    echo " CALLIOPE DIAGNOSTIC · COMPUTE                      M67 lane"
    echo "========================================================================"
    echo
    echo "---- checks ----------------------------------------------------------"
    echo "[FAIL] gpu-flavor build broken                           (M67 gate: the lane must build — cargo build --features gpu failed)"
    echo "CHECKS: 0 pass · 0 warn · 1 fail"
  } > "$OUT/compute.txt"
fi

# ---- the coast law's third executor (M67 follow-on, ADR-0027) --------------
# WGSL kernel and Rust twin prove byte-parity on lavapipe above. The JS
# port in render/compositor.js is the executor no device holds — so it
# answers the same law against the golden seed field the run just wrote.
# No golden (gpu-flavor build broken) or no bun is a FAIL, not a skip:
# the field is exported by the CPU twin, which every build carries.
echo "-- coast-js parity -> coastjs.txt"
if command -v bun >/dev/null 2>&1 && [ -f "$GOLDEN" ]; then
  bun ../../scripts/coast-js-parity.mjs "$GOLDEN" > "$OUT/coastjs.txt" || true
else
  {
    echo "========================================================================"
    echo " CALLIOPE DIAGNOSTIC · COAST-JS                     M67 third executor"
    echo "========================================================================"
    echo
    echo "---- checks ----------------------------------------------------------"
    echo "[FAIL] coast-js lane runs                                (M67 gate: the JS twin must be executable — no bun on PATH or no golden field exported)"
    echo "CHECKS: 0 pass · 0 warn · 1 fail"
  } > "$OUT/coastjs.txt"
fi




# ---- M22/M27 deep-earth replay across runtimes ------------------------------
# The same seed and months must yield one seismic ledger (M22) and one
# deep-earth identity line — plates, rock, seismic, volcanism, sealevel,
# landform (M27) — in native and in the shipped wasm. Skipped (not
# failed) when no wasm is built, bun is missing, or the binary predates
# the exports.
{
  echo "========================================================================"
  echo " CALLIOPE DIAGNOSTIC · DEEP-EARTH REPLAY         native vs shipped wasm"
  echo "========================================================================"
  RW="../web/js/wasm/calliope_bg.wasm"
  if [ -f "$RW" ] && command -v bun >/dev/null 2>&1; then
    NATIVE=$("$BIN" seismic-hash 777 "$SIZE" 240)
    NATIVE27=$("$BIN" earth-hash 777 "$SIZE" 240)
    set +e
    WASMH=$(bun ../../scripts/wasm-replay.mjs 777 "$SIZE" 240 2>/tmp/wasm-replay-err.txt | tail -1)
    RC=$?
    WASM27=$(bun ../../scripts/wasm-replay.mjs 777 "$SIZE" 240 earth 2>/tmp/wasm-replay27-err.txt | tail -1)
    RC27=$?
    set -e
    echo " seed 777 · size $SIZE · 240 mo"
    echo " native: $NATIVE"
    echo " native: $NATIVE27"
    if [ "$RC" -eq 3 ] || [ "$RC27" -eq 3 ]; then
      echo " wasm:   stale binary (missing replay export) — rebuild with scripts/build.sh"
      echo " (skipped, not failed)"
    elif [ "$RC" -ne 0 ] || [ "$RC27" -ne 0 ]; then
      echo " wasm:   replay run failed (rc=$RC/rc27=$RC27):"
      cat /tmp/wasm-replay-err.txt /tmp/wasm-replay27-err.txt 2>/dev/null | sed 's/^/   /' | head -5
      echo
      echo "---- checks ----------------------------------------------------------"
      echo "[FAIL] wasm replay runs                        rc=$RC/$RC27   (M22/M27 gate: the wasm leg must execute)"
      echo "CHECKS: 0 pass · 0 warn · 1 fail"
    else
      echo " wasm:   $WASMH"
      echo " wasm:   $WASM27"
      echo
      echo "---- checks ----------------------------------------------------------"
      P=0; F=0
      if [ "$NATIVE" = "$WASMH" ]; then
        echo "[PASS] seismic ledger agrees across runtimes        agree   (M22 gate: native and wasm replay one ledger)"
        P=$((P+1))
      else
        echo "[FAIL] seismic ledger agrees across runtimes     DIVERGE   (M22 gate: native and wasm replay one ledger)"
        F=$((F+1))
      fi
      if [ "$NATIVE27" = "$WASM27" ]; then
        echo "[PASS] deep-earth layers agree across runtimes      agree   (M27 gate: plates·rock·seismic·volcanism·sealevel·landform, one identity)"
        P=$((P+1))
      else
        echo "[FAIL] deep-earth layers agree across runtimes   DIVERGE   (M27 gate: the labeled line above names the layer)"
        F=$((F+1))
      fi
      echo "CHECKS: $P pass · 0 warn · $F fail"
    fi
  else
    echo " no wasm binary or no bun on PATH — cross-runtime leg not attempted"
    echo " (skipped, not failed)"
  fi
} > "$OUT/earth-wasm.txt"
echo "-- deep-earth replay (native vs wasm) -> earth-wasm.txt"

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
    # One law, one set of numbers: these bands are E6.4's (the canonical
    # statement, banded in build.txt by build.sh) — this row used to carry
    # a looser 3.2/4.0 copy, the silent-fork shape ADR-0026 closed for code.
    ROW=$(awk "BEGIN{printf \"%-48s %7s MiB\", \"wasm binary size\", \"$MIB\"}")
    if awk "BEGIN{exit !($MIB <= 3.0)}"; then
      echo "[PASS] $ROW   (sweet ≤3.0 MiB · hard ≤3.4 — E6.4 lean-binary budget)"
    elif awk "BEGIN{exit !($MIB <= 3.4)}"; then
      echo "[WARN] $ROW   (sweet ≤3.0 MiB · hard ≤3.4 — E6.4 lean-binary budget)"
    else
      echo "[FAIL] $ROW   (sweet ≤3.0 MiB · hard ≤3.4 — E6.4 lean-binary budget)"
    fi
    echo "CHECKS: see row above"
  else
    echo " no wasm binary at $WASM — build with scripts/build.sh first (skipped, not failed)"
  fi
} > "$OUT/wasm.txt"
echo "-- wasm size -> wasm.txt"

# ---- compile firewall (M65, staged by the M59 finding) ----------------------
# The M15 property lane kept hiding behind bin compile errors — a signature
# change breaks stagetest.rs or the assay's own harness and the lane
# reported [FAIL] for a law nobody disproved (three occurrences). Compile
# every target first as its own named row, so a build break names itself
# as a build break and the assay row can only ever fail as a counterexample.
echo "-- compile firewall (cargo check --all-targets) -> compile.txt"
{
  echo "========================================================================"
  echo " CALLIOPE DIAGNOSTIC · COMPILE                   every target, one gate"
  echo "========================================================================"
  if command -v cargo >/dev/null 2>&1; then
    CHECK_CMD=(cargo check --all-targets --quiet)
  else
    CHECK_CMD=(nix shell nixpkgs#rustc nixpkgs#cargo -c cargo check --all-targets --quiet)
  fi
  if "${CHECK_CMD[@]}" > /tmp/compile-out.txt 2>&1; then
    echo " lib · bins · tests: every target compiles"
    echo
    echo "---- checks ----------------------------------------------------------"
    echo "[PASS] every target compiles                             (M65 firewall: a build break names itself, never masquerades as a failed law)"
  else
    tail -30 /tmp/compile-out.txt | sed 's/^/ /'
    echo
    echo "---- checks ----------------------------------------------------------"
    echo "[FAIL] a target fails to compile                         (M65 firewall: fix the build, then believe the assay)"
  fi
  echo "CHECKS: see row above"
} > "$OUT/compile.txt"



# ---- the assay (M15) -------------------------------------------------------
# Property-proofs over the resource path: ontology forest, placement laws,
# price clamps, metamorphic market checks, conservation meters, hostile
# unpack. proptest hunts for the world that breaks a law; one failing case
# is a [FAIL] with its seed in the log. The assay builds under its own
# `assay` profile (panic=unwind, target/assay/): the release profile ships
# panic=abort (E6.2), and letting the two flavors share target/release/
# made them collide on one libcalliope.rlib path — the loser of that race
# linked against the wrong flavor and died with phantom undefined symbols.
echo "-- assay (property proofs) -> assay.txt"
{
  echo "========================================================================"
  echo " CALLIOPE DIAGNOSTIC · ASSAY                    M15 property proofs"
  echo "========================================================================"
  if command -v cargo >/dev/null 2>&1; then
    ASSAY_CMD=(cargo test --profile assay --test assay)
  else
    ASSAY_CMD=(nix shell nixpkgs#rustc nixpkgs#cargo -c cargo test --profile assay --test assay)
  fi
  if "${ASSAY_CMD[@]}" > /tmp/assay-out.txt 2>&1; then
    grep -E '^test |^test result' /tmp/assay-out.txt | sed 's/^/ /'
    echo
    echo "---- checks ----------------------------------------------------------"
    N=$(grep -c '^test .* \.\.\. ok$' /tmp/assay-out.txt || true)
    echo "[PASS] property lanes green                              ($N proofs — M15)"
  else
    tail -40 /tmp/assay-out.txt | sed 's/^/ /'
    echo
    echo "---- checks ----------------------------------------------------------"
    echo "[FAIL] property lane broken                              (M15: a law found its counterexample — read above)"
  fi
  echo "CHECKS: see row above"
} > "$OUT/assay.txt"

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

# ---- the era gate (M65) -----------------------------------------------------
# Runs last so it composes every report this run just wrote — the same
# rows SUMMARY greps, sealed into one verdict — plus a 300-year structural
# leg (60y in quick). One FAIL anywhere holds the era open, the honestly
# held rows included: the gate exists to see them, never to scope past them.
GATE_YEARS=300
[ "$MODE" = "quick" ] && GATE_YEARS=60
echo "-- era gate (compose + ${GATE_YEARS}y structural leg) -> gate.txt"
"$BIN" gate "$SIZE" "$GATE_YEARS" 12345 --reports "$OUT" > "$OUT/gate.txt"

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