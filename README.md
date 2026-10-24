# Scalable URL Shortener

A high-performance URL shortener in **Rust** (Tokio + Warp) with **Redis** storage, scaled out to three
backends behind an **Nginx** load balancer and packaged with **Docker Compose**.

| | Single backend | 3 backends behind Nginx |
|---|---|---|
| Throughput | **45,807 req/sec** | **44,230 req/sec** |
| Requests in 30 s | 1,374,404 | 1,331,499 |
| Concurrent connections | 100 | **7,000** |
| Avg / max latency | 2.22 ms / 32.16 ms | 216.94 ms / 8.93 s |

<sub>Apple M4 Mac mini, `POST /generate_url`. Full details in [Benchmarks](#benchmarks).</sub>

## Architecture

```
                    client
                      │
              ┌───────▼────────┐
              │  Nginx  :80    │  consistent hashing · 300 keepalive conns
              └──┬─────┬─────┬─┘
                 │     │     │
        ┌────────▼┐ ┌──▼─────┐ ┌▼────────┐
        │backend1 │ │backend2│ │backend3 │  Rust · Tokio · Warp
        │ :15555  │ │ :15556 │ │ :15557  │  Snowflake node 1 / 2 / 3
        └────┬────┘ └───┬────┘ └────┬────┘
             └──────────┼───────────┘
                  ┌─────▼──────┐
                  │ Redis :6379│  short code → long URL (with TTL)
                  └────────────┘
```

- **Snowflake IDs.** Each short code is a base62-encoded 64-bit ID:
  `41-bit ms timestamp | 10-bit node id | 12-bit sequence`. Every backend has its own `NODE_ID`, so
  codes are unique across the cluster with no coordination, at up to 4,096 IDs per ms per node.
- **Nginx load balancing.** `hash $remote_addr$remote_port consistent` spreads clients evenly and
  keeps each connection on the same backend. Only a small share of clients move if a backend is added
  or removed. A pool of 300 keepalive connections to the backends avoids opening a new TCP
  connection per request. Passive health checks (`max_fails=3`) retry on the next backend.
- **Lock-free Redis access.** Each backend shares one multiplexed async connection that pipelines
  commands from all in-flight requests.

## Tech stack

Rust · Tokio · Warp · Redis · Nginx · Docker Compose · Prometheus metrics · wrk · Next.js (optional UI)

## Getting started

Prerequisites: Docker with Docker Compose.

```bash
git clone https://github.com/Zekrom0506/scalable-url-shortener.git
cd scalable-url-shortener
docker compose up -d --build                      # Redis + 3 backends + Nginx
docker compose --profile frontend up -d --build   # optional: also the Next.js UI on :3000
```

The API key is set in `.env` (`API_KEY=123456789` by default).

```bash
curl -X POST http://localhost/generate_url \
  -H "API-Key: 123456789" -H "Content-Type: application/json" \
  -d '{"long_url": "https://example.com"}'
# {"short_url":"http://rustyshortener/dns_resolver/uobhIKepg8","status":"success"}
```

## API

| Method | Path | Body | Description |
|---|---|---|---|
| `POST` | `/generate_url` | `{"long_url": "..."}` | Create a short URL (needs `API-Key` header) |
| `POST` | `/custom_url` | `{"long_url": "...", "custom_short": "my-alias"}` | Create a custom alias (needs `API-Key`, returns 409 if taken) |
| `GET` | `/dns_resolver/:short_url` | | 307 redirect to the long URL |
| `GET` | `/ping` | | Health check |
| `GET` | `/metrics` | | Prometheus metrics |

## Benchmarks

**Setup:** Apple M4 Mac mini (10 cores), Docker Desktop with 10 CPUs / 8 GB, wrk 4.2.0 on the host.
Each request is `POST /generate_url` with a unique URL: it generates a Snowflake ID and writes to Redis
with `SETEX`. Run it yourself with `./run_benchmarks.sh`. Raw output is in [`benchmarks/`](benchmarks/).

### Single backend

```bash
wrk -t4 -c100 -d30s --latency -s wrk_post.lua http://127.0.0.1:15555/generate_url
```

| Throughput | Requests | Avg latency | p50 | p99 | Max | Errors |
|---|---|---|---|---|---|---|
| 45,807 req/sec | 1,374,404 | 2.22 ms | 2.11 ms | 4.31 ms | 32.16 ms | 0 |

### Nginx → 3 backends, 7,000 connections

```bash
wrk -t7 -c7000 -d30s --timeout 10s --latency -s wrk_post.lua http://127.0.0.1:80/generate_url
```

| Throughput | Requests | Avg latency | p50 | p99 | Non-2xx | Socket errors |
|---|---|---|---|---|---|---|
| 44,230 req/sec | 1,331,499 | 216.94 ms | 87.69 ms | 1.36 s | 0 | ~1.8% |

The three backends handled **463,660 / 455,633 / 417,979** requests: an even split.

**Note:** both runs were on a single machine, so all three backends, Redis, Nginx and wrk share the
same 10 cores. That's why total throughput stays about the same. On one machine, the load balancer's
gain is holding **70x the concurrent connections** (7,000 vs 100) at that throughput. On separate
hosts, each backend would add its own CPU.

## Tests

```bash
docker compose up -d redis
cd backend && cargo test
```

Includes Redis storage, TTL expiry and handler tests. There are also Snowflake uniqueness checks:
400,000 IDs generated concurrently on 8 threads, plus IDs from 3 nodes, with no duplicates.

## Roadmap

- Cassandra for persistent, replicated storage, with Redis as a hot-key cache
- Redis Cluster to remove the single shared Redis instance
- Kubernetes deployment with autoscaling
