# workdup

Temporal workflow history deduplication for replay-test optimization — maintains a live, deduplicated set of "unique" workflows (by semantic hash) so CI/QA replays a minimal covering set instead of every history.

## Observability dashboard

A Grafana dashboard is provisioned from [`docker/grafana/dashboards/workdup.json`](docker/grafana/dashboards/workdup.json) (title **"workdup — Observability"**). 

App metrics are exposed on `/metrics` (see `[http].port`); Temporal SDK gRPC metrics on `[http].temporal_metrics_port`.

### Dashboard variables

| Variable | Type | Default | Purpose |
| --- | --- | --- | --- |
| `$namespace` | query (multi-select) | `All` | Filters every panel by Temporal namespace. Populated from `label_values(scan_ticks_total, namespace)`. |
| `$scan_interval` | textbox | `600` | Your `scan_interval` **in seconds** (10m = `600`, 30m = `1800`, 60m = `3600`). Drives the red "falling-behind" threshold line on the *Scan tick duration* panel — set it to match your config (`workdup.toml`). |

### Panel: Scan tick duration (last, per namespace)

- **Metric:** `scan_tick_duration_seconds` — a **gauge**, labeled `namespace`, emitted in `scanner.rs` once per tick. It is the wall-clock time of the whole `scan()` call: list preferable workflows via the Visibility API → for each, compare `history_length`, fetch history, tokenize, hash, and upsert.
- **Queries:** `scan_tick_duration_seconds{namespace=~"$namespace"}` (one line per namespace) plus `vector($scan_interval)` rendered as the red dashed threshold line.
- **How to read it:** it's a gauge, so it shows the duration of the **most recent** scan and *holds that value* until the next tick — the line is **flat/stepped, not spiky**, and it does **not** drop to `0` between ticks. A flat `15s` means "the last scan took 15s." Compare it against the `$scan_interval` threshold: if the line approaches or crosses it, the scan isn't keeping up. (The scanner uses `MissedTickBehavior::Delay`, so an overrun silently *delays* the next scan and stretches data freshness — hence watching this against the interval is the primary "are we keeping up?")

- **Alert:**

  ```promql
  scan_tick_duration_seconds > 600   # last tick exceeded the (10m) interval → falling behind
  ```

  Match the number to your `scan_interval` (or reuse the same value you put in `$scan_interval`).

### Panel: Scan ticks (ok vs error, per 1h)

- **Metric:** `scan_ticks_total` — a **counter**, labeled `namespace` and `result` (`ok`/`error`), incremented once per tick in `scanner.rs`.
- **Query:** `sum by (namespace, result) (increase(scan_ticks_total{namespace=~"$namespace"}[1h]))` — number of scans in the last hour, split by result.
- **How to read it:** counters are monotonic (and reset to 0 on restart), so they're never plotted raw — `increase()` turns "total forever" into "how many happened in the window." The `error` line is the signal: any nonzero value means a scan tick failed (errors + Temporal `RESOURCE_EXHAUSTED` = throttling); the `ok` line just confirms ticks are still happening. 

**Alert:**

  ```promql
  increase(scan_ticks_total{result="error"}[1h]) > 0   # any failed scan in the last hour
  ```

### Panel: Workflow throughput (per tick)

- **Metrics:** `scan_workflows_listed` / `scan_workflows_processed` / `scan_workflows_updated` / `scan_workflows_skipped` — **gauges**, labeled `namespace`, set once at the end of each `scan()` in `scanner.rs` with that tick's totals. The matching `workflows_*_total` **counters** are still emitted for lifetime totals.
  - `listed` — workflows scanned via the Visibility API this tick
  - `processed` — new or history-changed (went through fetch → tokenize → hash)
  - `updated` — actually written to the DB (upsert)
  - `skipped` — unchanged (`history_length` matched), no history fetched
- **Queries:** the four gauges directly, e.g. `scan_workflows_listed{namespace=~"$namespace"}` (one line per metric per namespace).
- **How to read it:** these are **per-tick** counts, not per-second throughput — a gauge holds the last cycle's total, so the line is stepped (one point per tick) and stays flat between ticks. `listed ≈ processed + skipped`, and `updated ⊆ processed`; the **processed/listed ratio** shows how much churn each scan finds (mostly `skipped` = little changed since last tick = healthy steady state). Rising `processed`/`updated` means a burst of new or changed workflows. Per-tick gauges are used instead of `increase()` over a window because scan ticks are irregular (`MissedTickBehavior::Delay`), so any fixed window would alias or split a tick's counts — the gauge captures each cycle exactly.
- **Lifetime totals (optional stat):** use the counters over the dashboard range, e.g. `increase(workflows_processed_total{namespace=~"$namespace"}[$__range])`.
- **Alert:** mostly a diagnostic panel; a reasonable signal is "scans stopped finding anything" —
- **Tuning `scan_interval` with this panel:** the `processed` vs `skipped` split is a direct measure of *how often workflows actually change between ticks*, which is exactly the input for choosing `scan_interval` in `workdup.toml`. Read it like this:
  - Almost every tick is `skipped` with little/no `processed` → you're scanning **more often than workflows change**; raise `scan_interval` to cut load on Temporal with no real loss of freshness.
  - `processed`/`updated` is a large share of `listed` on most ticks → workflows change **faster than you scan**; lower `scan_interval` for a fresher deduped set (at the cost of more Visibility/history load).
  - Target a steady state that's mostly `skipped` with a small, steady `processed` trickle — that means the interval matches the real change rate. Re-check after load changes, since the right interval can differ per namespace (and `scan_interval` is per-namespace overridable).

### Panel: Workflows dropped (tokenization errors)

- **What it tells you:** how many workflows the scanner had to **skip** because it couldn't turn their history into a semantic hash — it hit a Temporal event type the tokenizer doesn't handle yet. A skipped workflow is **missing from the deduplicated set**, so your replay coverage has a blind spot. You want this at a **flat `0`**.
- **Query:** `sum(increase(workflows_dropped_total{namespace=~"$namespace"}[1h])) or vector(0)` — number of workflows dropped in the last hour. The `or vector(0)` keeps a green `0` line, so an empty panel means "healthy," not "metric missing."
- **How to read it:** `0` and flat → every workflow hashed cleanly, nothing to do. Any point above `0` → one or more workflows were dropped that hour, almost always because a new SDK/event variant showed up that the tokenizer doesn't cover.
- **What to do when it goes above 0:** the *reason* is deliberately **not** on the dashboard (putting it in a metric label would explode cardinality). Instead, open the **scanner logs** — every drop logs the full detail:

  ```
  level=error  msg="skipping workflow because semantic hash would be incomplete"
  workflow_id=... run_id=... error="Undefined type while trying to make hash string: <EventType>"
  ```

  Search the logs for `skipping workflow because semantic hash` (or filter by `workflow_id` / `run_id`). The `error=` field names the unhandled event type — add a rule for it in `tokenizer.rs` (see `spec/concept.md` §4.3.1), and the drops stop.
- **Alert:**

  ```promql
  increase(workflows_dropped_total{namespace=~"$namespace"}[1h]) > 0   # a workflow fell out of the dedup set → check scanner logs
  ```
