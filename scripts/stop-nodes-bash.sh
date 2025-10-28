#!/usr/bin/env bash
set -euo pipefail
# Best-effort stop of node processes launched from repo
if command -v pkill >/dev/null 2>&1; then
  pkill -f "target/release/node" || true
  pkill -f "target/release/node.exe" || true
else
  echo "pkill not found. You can close the node windows or kill processes manually."
fi
