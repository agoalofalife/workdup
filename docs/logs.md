# Logs

The JSON log format, filtering with `RUST_LOG`, and the lines that matter.

workdup writes one JSON object per line to **stdout**. There is no file logging and no
rotation — collect it with your container runtime or process supervisor.

```json
{"timestamp":"2026-08-01T13:17:57.653008Z","level":"INFO","fields":{"message":"http server listening on 0.0.0.0:8123"},"target":"workdup::http"}
{"timestamp":"2026-08-01T13:17:57.661874Z","level":"ERROR","fields":{"message":"worker exited with error","worker":"scanner","error":"Server connection error: ..."},"target":"workdup"}
{"timestamp":"2026-08-01T13:18:01.671974Z","level":"INFO","fields":{"message":"shutdown requested","signal":"SIGTERM"},"target":"workdup"}
```

## Envelope

| Key | Description |
| --- | --- |
| `timestamp` | UTC, RFC 3339 with microseconds |
| `level` | `TRACE`, `DEBUG`, `INFO`, `WARN` or `ERROR` |
| `target` | Emitting module, e.g. `workdup::scanner` |
| `fields.message` | The human-readable text |
| `fields.*` | Structured fields attached to that specific event |
| `span` | The innermost active span, when the event is inside one |
| `spans` | Every active span, outermost first |

Structured fields sit **inside** `fields`, alongside `message` — not at the top level. A
query for the drop line is `.fields.message` and `.fields.workflow_id`, not `.workflow_id`.

## Spans

Scanner and cleanup work runs inside a span carrying the namespace:

```text
span:  {"ns":"orders-prod","name":"scanner"}
```

Both worker types use the span name `scanner`, so filtering on the span name alone will not
separate scanner lines from cleanup lines. Use `target` for that — `workdup::scanner` versus
`workdup::cleanup`.

Lines emitted before a worker enters its span have no `span` key at all. `worker exited with
error` is the important case: a worker that fails to connect dies before the span is entered,
so that line carries no namespace. Its `worker` field says `scanner` or `cleanup`, but which
namespace failed is not in the line.

## `RUST_LOG`

Standard `tracing` filter syntax. Unset or unparseable falls back to `info`.

```bash
RUST_LOG=info
RUST_LOG=warn,workdup=debug          # debug for the app, warn for dependencies
RUST_LOG=workdup::scanner=debug      # one module
RUST_LOG=workdup::cleanup=trace
```

`debug` adds per-workflow history fetching from `workdup::temporal`, one line per page. On a
large namespace that is a lot of output.

## Lines worth knowing

### Lifecycle

| Level | Message | Fields |
| --- | --- | --- |
| INFO | `binding http to "0.0.0.0:8000"` | — |
| INFO | `http server listening on 0.0.0.0:8000` | — |
| INFO | `shutdown requested` | `signal` |
| INFO | `all workers stopped` | — |
| ERROR | `worker exited with error` | `worker`, `error` |

`worker exited with error` is the one to alert on. That thread does not come back, and the
process keeps serving HTTP without it — see [CLI reference](cli.md#workdup-run).

### Scanner

| Level | Message | Fields |
| --- | --- | --- |
| INFO | `Start scannig` | — |
| INFO | `scanned` | `workflow_id`, `wf_type` |
| INFO | `workflow updated in db` | `workflow_id`, `affected` |
| INFO | `cancellation requested - stopping scanning at safe point` | — |
| INFO | `scanner stopping` | — |
| ERROR | `scan tick failed` | `error` |
| ERROR | `skipping workflow because semantic hash would be incomplete` | `workflow_id`, `run_id`, `error` |

### Cleanup

| Level | Message | Fields |
| --- | --- | --- |
| INFO | `Start clean database` | — |
| INFO | `cleanup: removed completed workflow in temporal` | `workflow_id`, `status` |
| INFO | `cleanup: not found in temporal, removed state record` | `workflow_id`, `status` |
| INFO | `cleanup stopping` | — |
| WARN | `temporal unavailable, retrying next tick` | `workflow_id` |
| ERROR | `cleanup tick failed` | `error` |
| ERROR | `unexpected gRPC status` | `workflow_id`, `other`, `msg` |

`Start scannig` and `Featch history of workflow on page:` are spelled that way in the binary.
Grep for them as written.

## Dropped workflows

The line behind the *Workflows dropped* panel in [Metrics](metrics.md):

```json
{"level":"ERROR","fields":{"message":"skipping workflow because semantic hash would be incomplete","workflow_id":"order-4821","run_id":"0198f2c1-6a3e-7b21-9d4f-1c2e5a8b3f60","error":"Undefined type while trying to make hash string: <EventType>"},"target":"workdup::scanner"}
```

Every increment of `workflows_dropped_total` emits one of these. The `error` field names the
Temporal event type the tokenizer does not handle; the workflow is skipped and is therefore
absent from `/unique-workflows`, so replay coverage has a gap until a rule for that event type
is added to [`tokenizer.rs`](https://github.com/agoalofalife/workdup/blob/main/src/tokenizer.rs).

```bash
# every drop, with the unhandled event type
jq -r 'select(.fields.message | startswith("skipping workflow")) | .fields.error' < workdup.log | sort | uniq -c
```

## Startup failures

These go to stderr as a plain `Error:` line, not JSON, because they happen before or outside
the tracing setup:

| Output | Cause |
| --- | --- |
| `Error: read config workdup.toml` | No config file at that path |
| `Error: env var FOO was not defined in config` | A `${FOO}` reference with nothing to expand |
| `Error: parse config` | Malformed or unknown key — see [Configuration](configuration.md#unknown-keys-are-rejected) |
| `Error: Address already in use (os error 48)` | `[http].port` or `[http].temporal_metrics_port` is taken |
