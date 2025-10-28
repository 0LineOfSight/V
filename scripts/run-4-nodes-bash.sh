#!/usr/bin/env bash
set -euo pipefail

NODE="../../target/release/node.exe"
if [[ ! -x "$NODE" ]]; then
  NODE="../../target/release/node"
fi

if [[ ! -x "$NODE" ]]; then
  echo "Building release first..."
  cargo build --workspace --release
fi

mkdir -p run/n1 run/n2 run/n3 run/n4

# Avoid MSYS path conversion mangling of multiaddrs
export MSYS_NO_PATHCONV=1
export MSYS2_ENV_CONV_EXCL='P2P_LISTEN;P2P_BOOTSTRAP'

( cd run/n1 && \
  RPC_ADDR=127.0.0.1:8367 \
  QUIC_ADDR=127.0.0.1:7000 \
  P2P_LISTEN='/ip4/127.0.0.1/tcp/9000' \
  P2P_BOOTSTRAP='[]' \
  DB_PATH=./db1 \
  "$NODE" ) &

( cd run/n2 && \
  RPC_ADDR=127.0.0.1:8368 \
  QUIC_ADDR=127.0.0.1:7001 \
  P2P_LISTEN='/ip4/127.0.0.1/tcp/9001' \
  P2P_BOOTSTRAP='["/ip4/127.0.0.1/tcp/9000"]' \
  DB_PATH=./db2 \
  "$NODE" ) &

( cd run/n3 && \
  RPC_ADDR=127.0.0.1:8369 \
  QUIC_ADDR=127.0.0.1:7002 \
  P2P_LISTEN='/ip4/127.0.0.1/tcp/9002' \
  P2P_BOOTSTRAP='["/ip4/127.0.0.1/tcp/9000"]' \
  DB_PATH=./db3 \
  "$NODE" ) &

( cd run/n4 && \
  RPC_ADDR=127.0.0.1:8370 \
  QUIC_ADDR=127.0.0.1:7003 \
  P2P_LISTEN='/ip4/127.0.0.1/tcp/9003' \
  P2P_BOOTSTRAP='["/ip4/127.0.0.1/tcp/9000"]' \
  DB_PATH=./db4 \
  "$NODE" ) &

echo "Nodes started in background. To stop: bash scripts/stop-nodes-bash.sh"
