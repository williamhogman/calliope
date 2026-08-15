#!/usr/bin/env bash
# Build Calliope: compile the Rust simulation to WASM (if sources changed),
# then assemble the static site into dist/.
set -euo pipefail
cd "$(dirname "$0")/.."

RUST_DIR=game/rust
OUT=game/web/js/wasm

needs_build=0
if [ ! -f "$OUT/calliope_bg.wasm" ]; then
  needs_build=1
elif [ -n "$(find "$RUST_DIR/src" "$RUST_DIR/Cargo.toml" -newer "$OUT/calliope_bg.wasm" -print -quit 2>/dev/null)" ]; then
  needs_build=1
fi

if [ "$needs_build" = 1 ]; then
  echo "== rebuilding WASM engine =="
  if command -v wasm-pack >/dev/null 2>&1 && command -v cargo >/dev/null 2>&1; then
    (cd "$RUST_DIR" && wasm-pack build --target web --release)
  else
    nix shell nixpkgs#rustc nixpkgs#cargo nixpkgs#lld nixpkgs#binaryen nixpkgs#wasm-pack -c \
      bash -c "cd '$RUST_DIR' && wasm-pack build --target web --release"
  fi
  mkdir -p "$OUT"
  cp "$RUST_DIR/pkg/calliope.js" "$RUST_DIR/pkg/calliope_bg.wasm" "$OUT/"
else
  echo "== WASM engine up to date =="
fi

rm -rf dist
mkdir -p dist
cp -r game/web/. dist/
echo "== dist/ ready =="
