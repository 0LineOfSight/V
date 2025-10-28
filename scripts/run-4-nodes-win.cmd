@echo off
setlocal enabledelayedexpansion
REM Start 4 nodes with distinct ports; run from repo root
set NODE=target\release\node.exe
if not exist "%NODE%" (
  echo Building release first...
  cargo build --workspace --release || goto :eof
)

mkdir run\n1 2>nul
mkdir run\n2 2>nul
mkdir run\n3 2>nul
mkdir run\n4 2>nul

start "node n1" cmd /c "cd /d run\n1 && set RPC_ADDR=127.0.0.1:8367 && set QUIC_ADDR=127.0.0.1:7000 && set P2P_LISTEN=/ip4/127.0.0.1/tcp/9000 && set P2P_BOOTSTRAP=[] && set DB_PATH=./db1 && ..\..\%NODE%"
start "node n2" cmd /c "cd /d run\n2 && set RPC_ADDR=127.0.0.1:8368 && set QUIC_ADDR=127.0.0.1:7001 && set P2P_LISTEN=/ip4/127.0.0.1/tcp/9001 && set P2P_BOOTSTRAP=[\"/ip4/127.0.0.1/tcp/9000\"] && set DB_PATH=./db2 && ..\..\%NODE%"
start "node n3" cmd /c "cd /d run\n3 && set RPC_ADDR=127.0.0.1:8369 && set QUIC_ADDR=127.0.0.1:7002 && set P2P_LISTEN=/ip4/127.0.0.1/tcp/9002 && set P2P_BOOTSTRAP=[\"/ip4/127.0.0.1/tcp/9000\"] && set DB_PATH=./db3 && ..\..\%NODE%"
start "node n4" cmd /c "cd /d run\n4 && set RPC_ADDR=127.0.0.1:8370 && set QUIC_ADDR=127.0.0.1:7003 && set P2P_LISTEN=/ip4/127.0.0.1/tcp/9003 && set P2P_BOOTSTRAP=[\"/ip4/127.0.0.1/tcp/9000\"] && set DB_PATH=./db4 && ..\..\%NODE%"

echo Started nodes in 4 windows. Press any key to exit this starter script...
pause >nul
