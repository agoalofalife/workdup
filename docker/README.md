# Local observability stack

Builds and runs `workdup` alongside Prometheus and Grafana so you can watch the
metrics from [`spec/observability.md`](../spec/observability.md) in a dashboard
while debugging locally.

```
┌──────────┐  scrape :8000/metrics (app)      ┌────────────┐   ┌─────────┐
│ workdup  │◀─────────────────────────────────│ Prometheus │◀──│ Grafana │
│ :8000    │  scrape :9000/metrics (Temporal) │  :9090     │   │  :3000  │
│ :9000    │◀─────────────────────────────────│            │   │         │
└──────────┘                                  └────────────┘   └─────────┘
```

## Run

```bash
docker compose up --build
```

| Service    | URL                          | Notes                                        |
|------------|------------------------------|----------------------------------------------|
| Grafana    | http://localhost:3000        | anonymous admin; dashboard auto-provisioned  |
| Prometheus | http://localhost:9090        | check **Status → Targets** are `UP`          |
| App API    | http://localhost:8000        | `/metrics`, `/healthz`, `/readyz`, `/stats`  |
| Temporal   | http://localhost:9000/metrics| SDK gRPC metrics (`request*`, `long_request*`)|

The dashboard is under **Dashboards → workdup — Observability** with a
`namespace` selector at the top.

## What it mounts

- `wd.toml` → `/app/workdup.toml` — the config the binary reads.
- `cert.pem` / `key.pem` → `/app/certs/` — TLS client certs for Temporal Cloud.
  The compose file overrides `TEMPORAL_TLS_CLIENT_CERT_PATH` /
  `TEMPORAL_TLS_CLIENT_KEY_PATH` to these in-container paths. **Replace the repo's
  self-signed `cert.pem`/`key.pem` with your real Temporal Cloud certs** (edit
  the bind mounts in `docker-compose.yml`) for the scanner to connect.
- `.env` — supplies `RUST_LOG` and any secrets.
- Named volumes persist the SQLite DB, Prometheus TSDB, and Grafana state.

## Two scrape targets, on purpose

`workdup` exposes metrics on **two** ports (see `spec/observability.md` §3):

- `:8000/metrics` — application metrics via `metrics-exporter-prometheus`.
  Histograms are rendered as **summaries**, so latency panels read the
  `quantile="0.5|0.9|0.99"` series directly (no `histogram_quantile`).
- `:9000/metrics` — Temporal SDK gRPC meter via `start_prometheus_metric_exporter`,
  with real histogram buckets and `_seconds`/`_total` suffixes.

## Notes

- The **Process / runtime** row (`process_cpu_seconds_total`, RSS, fds) stays
  empty until the `metrics-process` collector is wired in (§4.6) — the panels are
  pre-built for when it is.
- If the Temporal gRPC bucket metric is named differently on your SDK build,
  adjust the `*_latency_seconds_bucket` expressions in the last row.
- Editing `docker/grafana/dashboards/workdup.json` reloads within ~10s (no
  restart needed).
