#!/usr/bin/env bash
set -euo pipefail
cd /dev-server
printf 'closure start %s pid=%s\n' "$(date -u +%FT%TZ)" "$$"
./scripts/build.sh
./scripts/wasm-audit.sh
cd game/rust
./scripts/report.sh full
printf 'closure complete %s pid=%s\n' "$(date -u +%FT%TZ)" "$$"
