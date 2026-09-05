#!/usr/bin/env bash
set -euo pipefail
cd /dev-server
printf 'M96 closure start %s pid=%s\n' "$(date -u +%FT%TZ)" "$$"
./scripts/build.sh
./scripts/wasm-audit.sh
cd game/rust
./scripts/report.sh full
printf 'M96 closure complete %s pid=%s\n' "$(date -u +%FT%TZ)" "$$"
