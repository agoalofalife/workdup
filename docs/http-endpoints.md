# HTTP endpoints

All five routes, with parameters, status codes and real response bodies.

Everything below is served by one axum server on `[http].port` (default `8000`), bound to
`0.0.0.0`. There is no authentication and no TLS — do not expose it outside your network.

| Route | Method | Query | Returns |
| --- | --- | --- | --- |
| `/healthz` | GET | — | `ok` |
| `/readyz` | GET | — | Empty body, or the failure reason |
| `/unique-workflows` | GET | `namespace` (required) | JSON array of workflows |
| `/stats` | GET | `namespace` (required) | JSON array with one object |
| `/metrics` | GET | — | Prometheus text format |

## `GET /unique-workflows`

The deduplicated covering set for one namespace — one workflow per distinct semantic hash.
This is the endpoint a replay suite consumes.

```console
$ curl 'localhost:8000/unique-workflows?namespace=demo'
```

```json
[
  {
    "workflow_id": "refund-118",
    "run_id": "0198f2c2-8c50-7d43-9f61-3e4a7c0d5b82",
    "workflow_type": "RefundSaga"
  },
  {
    "workflow_id": "order-4821",
    "run_id": "0198f2c1-6a3e-7b21-9d4f-1c2e5a8b3f60",
    "workflow_type": "OrderFulfillment"
  }
]
```

Order is not specified — it is whatever SQLite returns for a `GROUP BY semantic_hash`. Treat
the result as a set.

## `GET /stats`

Row counts for one namespace.

```console
$ curl 'localhost:8000/stats?namespace=demo'
[{"workflows_count":3,"unique_workflows_count":2}]
```

It returns an **array containing exactly one object**, not a bare object — index `[0]`, or
`jq '.[0]'`. An unknown namespace returns the same shape with zeros rather than an empty
array:

```console
$ curl 'localhost:8000/stats?namespace=nope'
[{"workflows_count":0,"unique_workflows_count":0}]
```

## `GET /healthz`

```console
$ curl localhost:8000/healthz
ok
```

A static response. It proves the HTTP server is accepting connections and nothing else — in
particular it stays `200` when every scanner and cleanup worker has died. Use it as a
liveness probe, and alert on the scanner metrics for actual progress.

## `GET /readyz`

Opens the database, runs `SELECT 1`, then opens a **fresh gRPC connection to every configured
namespace**. Returns `200` with an empty body when all of that succeeds, or `503` with the
first failure as plain text:

```console
$ curl localhost:8000/readyz
not ready: Server connection error: tonic::transport::Error(Transport, ConnectError(
ConnectError("tcp connect error", 127.0.0.1:7233, Os { code: 61, kind: ConnectionRefused,
message: "Connection refused" })))
```

## `GET /metrics`

Prometheus text format: application metrics recorded by the code, plus `process_*` collected
on each scrape.

```console
$ curl -s localhost:8000/metrics | head -6
# HELP process_cpu_seconds_total Total user and system CPU time spent in seconds.
# TYPE process_cpu_seconds_total counter
process_cpu_seconds_total 0

# TYPE http_requests_total counter
http_requests_total{route="/healthz",method="GET",status="200"} 1
```

Temporal SDK gRPC-client metrics are **not** here. They are on a second port,
`[http].temporal_metrics_port` (default `9000`), served by the SDK's own exporter. That
endpoint responds `200` with an empty body until the SDK has made its first call. See
[Metrics](metrics.md).

Every request through the router is counted, so the RED panels cover all five routes:

```text
http_requests_total{route, method, status}
http_request_duration_seconds{route}
```

## Status codes

| Code | Body | When |
| --- | --- | --- |
| `200` | JSON, or text for `/healthz` and `/metrics` | Success |
| `400` | `Failed to deserialize query string: missing field \`namespace\`` | The `namespace` query parameter is absent |
| `404` | Empty | No such route |
| `500` | `error: <message>`, plain text | A database error. The body is the internal error string, not JSON |
| `503` | `not ready: <message>`, plain text | `/readyz` only |

Error bodies are plain text even on the JSON endpoints — check the status code before parsing.
