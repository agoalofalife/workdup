# Configuration

The `workdup.toml` file: sections, parameters, overrides and secrets.

The file is not secret. It declares structure — namespaces, hosts, intervals, and the *locations* of credentials. Certificates are referenced by path; tokens are referenced by environment variable. Neither is ever stored in the file, so it can be committed or shipped as a Kubernetes ConfigMap.

## Where the file comes from

| Source                                   | Precedence |
| ---------------------------------------- | ---------- |
| `--config <PATH>`                        | Highest    |
| `WORKDUP_CONFIG` in the real environment |            |
| `WORKDUP_CONFIG` in a `.env` file        |            |
| `./workdup.toml`                         | Default    |

A relative path resolves against the current working directory — see [Installation](https://agoalofalife.github.io/workdup/dev/installation/#working-directory-matters).

## Load order

```text
TOML file  →  ${VAR} expansion  →  WORKDUP__* environment overrides
```

`${VAR}` expansion happens on the raw file text, before it is parsed as TOML. Environment overrides are merged after parsing and win over the file.

## Example

The file below is [`wd.example.toml`](https://github.com/agoalofalife/workdup/blob/main/wd.example.toml) from the repository, included verbatim:

wd.example.toml

```toml
[defaults]
scan_interval    = "1h"                              # how often scan temporal cluster for new updates
cleanup_interval = "1d"                              # how often clean outdated workflow in storage
query            = "ExecutionStatus = 'Running'"     # query for get deduplication workflow

[http]
port = 8000
temporal_metrics_port = 9000

[[namespaces]]
name = "namespace-1"
host = "host.cloud:7233"
tls  = { cert_path = "${TEMPORAL_TLS_CLIENT_CERT_PATH}", key_path = "${TEMPORAL_TLS_CLIENT_KEY_PATH}" }

[[namespaces]]
name = "namespace-2"
host = "host.cloud:7233"
scan_interval = "10m"                               # we can redefine individual setting for namespace
tls  = { cert_path = "${TEMPORAL_TLS_CLIENT_CERT_PATH}", key_path = "${TEMPORAL_TLS_CLIENT_KEY_PATH}" }
```

## `[defaults]`

Base values for every namespace. Each one can be overridden inside an individual `[[namespaces]]` entry.

| Key                | Type     | Default                       | Description                                            |
| ------------------ | -------- | ----------------------------- | ------------------------------------------------------ |
| `scan_interval`    | duration | `1h`                          | How often the scanner ticks                            |
| `cleanup_interval` | duration | `1d`                          | How often stale rows are removed                       |
| `query`            | string   | `ExecutionStatus = 'Running'` | Temporal List filter selecting which workflows to scan |

## `[http]`

One HTTP server per process; these are not overridable per namespace.

| Key                     | Type | Default | Description                                              |
| ----------------------- | ---- | ------- | -------------------------------------------------------- |
| `port`                  | u16  | `8000`  | REST API and application `/metrics`                      |
| `temporal_metrics_port` | u16  | `9000`  | Prometheus exporter for Temporal SDK gRPC-client metrics |

The two ports serve different metric sets from different servers. Application metrics (`scan_*`, `db_*`, `http_*`) are rendered by the API server on `port`; the Temporal SDK starts its own exporter on `temporal_metrics_port`. Both are always enabled. See [Metrics](https://agoalofalife.github.io/workdup/dev/metrics/index.md).

## `[[namespaces]]`

One entry per Temporal namespace.

| Key                | Type     | Required | Description                                    |
| ------------------ | -------- | -------- | ---------------------------------------------- |
| `name`             | string   | yes      | Temporal namespace name                        |
| `host`             | string   | yes      | Frontend address, `host:port` or a full URL    |
| `api_key`          | string   | no       | Temporal Cloud API key. Enables TLS on its own |
| `tls.cert_path`    | path     | no¹      | Client certificate (mTLS)                      |
| `tls.key_path`     | path     | no¹      | Private key                                    |
| `tls.ca_path`      | path     | no       | CA certificate, if not in the system store     |
| `scan_interval`    | duration | no       | Overrides `[defaults].scan_interval`           |
| `cleanup_interval` | duration | no       | Overrides `[defaults].cleanup_interval`        |
| `query`            | string   | no       | Overrides `[defaults].query`                   |

¹ Required together for Temporal Cloud with mTLS. Omit the whole `tls` block for a local dev server without TLS.

TLS is enabled when either `tls` or `api_key` is present. When `host` has no scheme, `https://` is prepended if TLS is on and `http://` otherwise.

Durations use `humantime` syntax: `30s`, `10m`, `1h`, `1d`. An unparseable value is a startup error, not a fallback.

## Referencing environment variables

Any string value may contain `${VAR}`, expanded from the environment at load time:

```toml
[[namespaces]]
name    = "tenant-a"
host    = "tenant-a.acme.tmprl.cloud:7233"
api_key = "${TENANT_A_API_KEY}"
tls     = { cert_path = "${SECRETS_DIR}/tls.crt", key_path = "${SECRETS_DIR}/tls.key" }
```

Expansion is strict. An unset variable aborts startup naming the variable — it never substitutes an empty string:

```console
$ workdup --config workdup.toml validate
Error: env var TENANT_A_API_KEY was not defined in config
```

Because expansion runs over the whole file before TOML parsing, a literal `$` anywhere in the file is treated as the start of a reference.

## Overriding from the environment

Any scalar can be overridden with a `WORKDUP__` variable, using `__` as the section separator:

```bash
WORKDUP__HTTP__PORT=8080 workdup run
```

Note the double underscore. `WORKDUP_CONFIG` uses a single underscore and is a different mechanism — a CLI flag fallback, not a config overlay. This overlay suits scalars in `[defaults]` and `[http]`; the `[[namespaces]]` array is better expressed in the file with `${VAR}` references for its secrets.

## Unknown keys are rejected

Every config struct sets `deny_unknown_fields`, so a typo fails at startup instead of being silently ignored. This applies to the file:

```console
$ workdup --config workdup.toml validate
Error: parse config

Caused by:
    unknown field: found `bogus`, expected ``port` or `temporal_metrics_port`` for key "default.http.bogus" in TOML source string
```

and to the environment overlay:

```console
$ WORKDUP__NONSENSE=1 workdup validate
Error: parse config

Caused by:
    unknown field: found `nonsense`, expected `one of `defaults`, `http`, `namespaces`` for key "NONSENSE" in `WORKDUP__` environment variable(s)
```

The message lists the keys that were expected, which is what makes a misspelling self-diagnosing.

## Checking a file

[`workdup validate`](https://agoalofalife.github.io/workdup/dev/cli/#workdup-validate) loads the config, applies everything above, and verifies that at least one namespace exists, that no name is duplicated, that no host is empty, and that every TLS path is a real file. It does not connect to Temporal and does not touch the database.
