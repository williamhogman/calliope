#!/usr/bin/env bash
# E6.4/E6.6 — wasm size audit. The shipped release binary is stripped
# (no name section), so twiggy would only see anonymous code. This script
# builds a symbolized twin with --profiling into game/rust/pkg-prof/ and
# lets twiggy name the heaviest items; the report lands in
# game/reports/wasm-audit.txt for reading after any size regression.
#
# M65 follow-up: the audit carries measured check rows so the era gate
# composes its evidence instead of prose. Its staleness law is content,
# not clock — the report records the subject binary's sha256, and
# `diagnose gate` holds a must-row against the currently shipped bytes.
set -euo pipefail
cd "$(dirname "$0")/.."

RUST_DIR=game/rust
OUT=game/reports/wasm-audit.txt

echo "== building symbolized twin (--profiling) =="
if command -v wasm-pack >/dev/null 2>&1 && command -v cargo >/dev/null 2>&1; then
  (cd "$RUST_DIR" && wasm-pack build --target web --profiling --out-dir pkg-prof)
else
  nix shell nixpkgs#rustc nixpkgs#cargo nixpkgs#lld nixpkgs#binaryen nixpkgs#wasm-pack -c \
    bash -c "cd '$RUST_DIR' && wasm-pack build --target web --profiling --out-dir pkg-prof"
fi

REL=game/web/js/wasm/calliope_bg.wasm
PROF=$RUST_DIR/pkg-prof/calliope_bg.wasm
TWIG=/tmp/twiggy-top.txt
if command -v twiggy >/dev/null 2>&1; then
  twiggy top -n 40 "$PROF" > "$TWIG"
else
  nix run nixpkgs#twiggy -- top -n 40 "$PROF" > "$TWIG"
fi

RELB=$(wc -c < "$REL" | tr -d ' ')
PROFB=$(wc -c < "$PROF" | tr -d ' ')
SHA=$(sha256sum "$REL" | awk '{print $1}')
# bytes of the name section (symbolization overhead, absent from shipped)
NAMES=$(awk -F'┊' '/"function names" subsection/ {gsub(/[^0-9]/, "", $1); print $1; exit}' "$TWIG")
: "${NAMES:=0}"
# heaviest real item: first data row that is not the names subsection
TOP_BYTES=$(awk -F'┊' 'NF >= 3 && $1 ~ /[0-9]/ && $3 !~ /function names/ {gsub(/[^0-9]/, "", $1); print $1; exit}' "$TWIG")
TOP_LABEL=$(awk -F'┊' 'NF >= 3 && $1 ~ /[0-9]/ && $3 !~ /function names/ {gsub(/^[ \t]+|[ \t]+$/, "", $3); print substr($3, 1, 46); exit}' "$TWIG")
: "${TOP_BYTES:=0}"

# twin-integrity: after removing the name section, the twin must be the
# shipped code give or take profiling codegen noise (measured ~1.7%).
DELTA=$(awk "BEGIN{d = ($PROFB - $NAMES - $RELB) / $RELB * 100; if (d < 0) d = -d; printf \"%.1f\", d}")
TOP_PCT=$(awk "BEGIN{printf \"%.1f\", $TOP_BYTES / $PROFB * 100}")

lvl() { # lvl <value> <sweet> <hard>  →  PASS/WARN/FAIL by upper bound
  awk "BEGIN{v=$1; if (v <= $2) print \"PASS\"; else if (v <= $3) print \"WARN\"; else print \"FAIL\"}"
}
L1=$(lvl "$DELTA" 3.0 6.0)
L2=$(lvl "$TOP_PCT" 8.0 12.0)

{
  echo "========================================================================"
  echo " CALLIOPE DIAGNOSTIC · WASM AUDIT                   twiggy top (E6.4)"
  echo "========================================================================"
  echo " subject: calliope_bg.wasm $RELB B · sha256 $SHA"
  echo " profiling twin (symbolized): $PROFB B — sizes below are from the"
  echo " twin; the release binary is the same code without names."
  echo
  cat "$TWIG"
  echo
  echo "---- checks ----------------------------------------------------------"
  printf '[%s] %-36s %14s   (%s)\n' "$L1" "twin speaks for the shipped binary" "Δ ${DELTA}%" \
    "E6.6: |twin − names − release| ≤3% sweet · ≤6% hard of release — beyond that the audit reads a different build"
  printf '[%s] %-36s %14s   (%s)\n' "$L2" "no single item dominates the twin" "${TOP_PCT}%" \
    "E6.6: heaviest item ≤8% sweet · ≤12% hard — today's worst is the ~3.4% dawn stage; a jump names a new monster: ${TOP_LABEL}"
  P=0; W=0; F=0
  for l in "$L1" "$L2"; do
    case "$l" in PASS) P=$((P+1));; WARN) W=$((W+1));; FAIL) F=$((F+1));; esac
  done
  echo "CHECKS: $P pass · $W warn · $F fail"
} > "$OUT"
echo "== audit written to $OUT =="
tail -n +8 "$OUT" | head -34
