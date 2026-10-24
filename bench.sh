#!/usr/bin/env bash
# Reproduce the benchmarks. Run on the host (not in Docker) after:
#   docker compose up -d --build
# Results are written to ./benchmarks/
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p benchmarks
ulimit -n 65535 2>/dev/null || ulimit -n "$(ulimit -Hn)"

echo "Warming up..."
wrk -t4 -c100 -d5s -s wrk_post.lua http://127.0.0.1:15555/generate_url >/dev/null
wrk -t4 -c100 -d5s -s wrk_post.lua http://127.0.0.1:80/generate_url >/dev/null

echo "== Single backend (port 15555), 100 connections =="
wrk -t4 -c100 -d30s --latency -s wrk_post.lua http://127.0.0.1:15555/generate_url \
  | tee benchmarks/single_backend.txt

count() { curl -s "http://127.0.0.1:$1/metrics" | awk '/^generate_url_requests_total/{print $2}'; }
b1=$(count 15555); b2=$(count 15556); b3=$(count 15557)

sleep 5
echo "== Nginx load balancer -> 3 backends (port 80), 7000 connections =="
wrk -t7 -c7000 -d30s --timeout 10s --latency -s wrk_post.lua http://127.0.0.1:80/generate_url \
  | tee benchmarks/load_balanced.txt

echo "== Requests handled per backend during the load-balanced run ==" | tee benchmarks/distribution.txt
for p in 15555 15556 15557; do
  case $p in 15555) b=$b1;; 15556) b=$b2;; 15557) b=$b3;; esac
  echo "backend :$p -> $(awk -v a="$(count $p)" -v b="$b" 'BEGIN{printf "%d", a-b}')"
done | tee -a benchmarks/distribution.txt
