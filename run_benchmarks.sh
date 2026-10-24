#!/usr/bin/env bash
# One-shot: install wrk, build + start the stack, run both benchmarks on this Mac.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p benchmarks

if ! command -v wrk >/dev/null; then
  command -v brew >/dev/null || { echo "Homebrew not found: install it from https://brew.sh then re-run"; exit 1; }
  brew install wrk
fi

{
  echo "Host: $(sysctl -n machdep.cpu.brand_string) ($(sysctl -n hw.ncpu) cores), macOS $(sw_vers -productVersion)"
  echo "Docker: $(docker info --format '{{.NCPU}} CPUs, {{.MemTotal}} bytes RAM, server {{.ServerVersion}}')"
  echo "wrk: $(wrk -v 2>&1 | head -1)"
} | tee benchmarks/environment.txt

docker compose down --remove-orphans >/dev/null 2>&1 || true
docker compose up -d --build
echo "Waiting for backends..."
for p in 15555 15556 15557 80; do
  until curl -fs "http://127.0.0.1:$p/ping" >/dev/null; do sleep 1; done
done
curl -s -XPOST http://127.0.0.1/generate_url -H 'API-Key: 123456789' \
  -H 'Content-Type: application/json' -d '{"long_url":"https://example.com"}'; echo

./bench.sh

echo; echo "DONE - results are in ./benchmarks/. You can go back to Claude now."
