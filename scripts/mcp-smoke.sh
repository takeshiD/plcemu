#!/bin/sh
# Connects to `plcemu mcp` over stdio, lists tools and runs a simulation.
set -eu
BIN=${1:-./target/debug/plcemu}
OUT=$(printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"simulate","arguments":{"program":"examples/motor.plc.json","scan_ms":100,"scans":21,"inputs":[{"scan":0,"device":"X0","value":1}]}}}' \
  | "$BIN" mcp)
echo "$OUT"
echo "$OUT" | grep -q '"name":"simulate"'
echo "$OUT" | grep -q '"name":"run_tests"'
echo "$OUT" | grep -q '\\"Y1\\": 1'
echo "MCP smoke OK"
