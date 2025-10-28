Scripts to run 4 local nodes and benchmark them.

Contents
--------
scripts/
  run-4-nodes-win.cmd        # Start 4 nodes (use cmd.exe)
  stop-nodes-win.cmd         # Stop nodes (force-kill node.exe)
  run-4-nodes-bash.sh        # Start 4 nodes (Git Bash / MSYS2 / WSL)
  stop-nodes-bash.sh         # Stop nodes from bash
  bench-examples.txt         # Ready-to-copy benchmark commands
  smoke-test-bash.sh         # Quick smoke test (balance + 100 tx)

Usage (Windows cmd.exe)
-----------------------
1) Extract this zip into your repo root.
2) Build release:
      cargo build --workspace --release
3) Start nodes:
      scripts\run-4-nodes-win.cmd
4) Run a benchmark (example):
      cargo run -p bench --release -- --n 5000 --concurrency 128 --url http://127.0.0.1:8367 --from alice --to bench-bob --csv e2e_c128.csv
5) Stop nodes:
      scripts\stop-nodes-win.cmd

Usage (Git Bash / MSYS2 bash)
-----------------------------
1) Extract this zip into your repo root.
2) Build release:
      cargo build --workspace --release
3) Start nodes:
      bash scripts/run-4-nodes-bash.sh
4) Benchmark:
      cargo run -p bench --release -- --n 5000 --concurrency 128 --url http://127.0.0.1:8367 --from alice --to bench-bob --csv e2e_c128.csv
5) Stop nodes:
      bash scripts/stop-nodes-bash.sh

Notes
-----
- The scripts use distinct RPC/QUIC/P2P ports to avoid conflicts:
    Node1: RPC 8367, QUIC 7000, P2P /ip4/127.0.0.1/tcp/9000 (seed)
    Node2: RPC 8368, QUIC 7001, P2P /ip4/127.0.0.1/tcp/9001 (bootstraps to Node1)
    Node3: RPC 8369, QUIC 7002, P2P /ip4/127.0.0.1/tcp/9002 (bootstraps to Node1)
    Node4: RPC 8370, QUIC 7003, P2P /ip4/127.0.0.1/tcp/9003 (bootstraps to Node1)

- Databases live in run/n*/db* directories. Remove them between runs if you want a fresh chain.

- If you run from Git Bash, the script sets MSYS_NO_PATHCONV and MSYS2_ENV_CONV_EXCL to prevent multiaddr mangling.

- If Windows Firewall prompts for access on first run, allow it for local loopback to avoid connection issues.
