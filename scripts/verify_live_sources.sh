#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_DIR="${XMEDIA_E2E_REPORT_DIR:-$REPO_ROOT/e2e}"
XMEDIA_BIN="${XMEDIA_BIN:-$REPO_ROOT/target/debug/xmedia}"

if [[ ! -x "$XMEDIA_BIN" ]]; then
  echo "Building xmedia binary..."
  cargo build --manifest-path "$REPO_ROOT/Cargo.toml" --bin xmedia
fi

exec node "$REPO_ROOT/e2e/run.mjs" --live --xmedia-bin "$XMEDIA_BIN" --live-report "$REPORT_DIR" "$@"
