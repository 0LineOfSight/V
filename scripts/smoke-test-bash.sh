#!/usr/bin/env bash
set -euo pipefail
URL=${1:-http://127.0.0.1:8367}
echo "Balance(alice) before:"
curl -s "$URL/balance/alice" || true; echo
cargo run -p bench --release -- --n 100 --concurrency 16 --url "$URL" --from alice --to bench-bob --csv smoke.csv
echo "Balance(alice) after:"
curl -s "$URL/balance/alice" || true; echo
