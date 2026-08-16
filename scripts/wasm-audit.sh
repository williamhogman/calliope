#!/usr/bin/env bash
# E6.4/E6.6 — wasm size audit. The shipped release binary is stripped
# (no name section), so twiggy would only see anonymous code. This script
# builds a symbolized twin with --profiling into game/rust/pkg-prof/ and
# lets twiggy name the heaviest items; the report lands in
# game/reports/wasm-audit.txt for reading after any size regression.
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
{
  echo "========================================================================"
  echo " CALLIOPE DIAGNOSTIC · WASM AUDIT                   twiggy top (E6.4)"
  echo "========================================================================"
  echo " release (stripped, shipped): $(wc -c < "$REL") B"
  echo " profiling twin (symbolized): $(wc -c < "$PROF") B — sizes below are"
  echo " from the twin; the release binary is the same code without names."
  echo
  if command -v twiggy >/dev/null 2>&1; then
    twiggy top -n 40 "$PROF"
  else
    nix run nixpkgs#twiggy -- top -n 40 "$PROF"
  fi
} > "$OUT"
echo "== audit written to $OUT =="
tail -n +8 "$OUT" | head -30
