# CLI reference

Every command, argument and flag, with its environment fallback.

```text
workdup [--config <PATH>] <COMMAND>
```

Two commands: `run` starts the daemon, `validate` checks the config and exits. Both read the
same config file and both resolve paths relative to the **current working directory** — see
[Installation](installation.md#working-directory-matters).

## Global options

| Flag | Environment fallback | Default | Purpose |
| --- | --- | --- | --- |
| `--config <PATH>` | `WORKDUP_CONFIG` | `workdup.toml` | Path to the config file. A relative path resolves against the working directory |
| `-h`, `--help` | — | — | Print help. Works on the subcommands too: `workdup help run` |
| `-V`, `--version` | — | — | Print `workdup <version>` and exit |

Precedence for the config path, highest first: `--config`, then `WORKDUP_CONFIG` from the real
environment, then `WORKDUP_CONFIG` from a `.env` file, then the default.

## `workdup validate`

Loads the config, expands `${VAR}` references, applies any `WORKDUP__*` overrides, checks it,
and exits.

```console
$ workdup --config workdup.toml validate
ok: 2 namespace(s)
```

It checks four things:

- at least one namespace is defined;
- no namespace name appears twice;
- no `host` is empty;
- every TLS path in the config — `cert_path`, `key_path`, and `ca_path` if set — exists as a
  real file.

## `workdup run`

Starts the daemon. It initialises the SQLite schema at `./data/workdup.db`, creating `data/`
if needed, then spawns:

- one **scanner** thread per namespace, ticking every `scan_interval`;
- one **cleanup** thread per namespace, ticking every `cleanup_interval`;
- one **HTTP server** for the whole process, on `[http].port`;
- one **Temporal SDK metrics exporter**, on `[http].temporal_metrics_port`.

Each worker gets its own thread and its own single-threaded Tokio runtime. The process runs
until `SIGINT` or `SIGTERM`, then cancels a shared token, waits for every worker to finish its
current tick, and logs `all workers stopped`. `SIGTERM` is handled explicitly so a Kubernetes
rollout shuts down cleanly instead of being `SIGKILL`ed at the end of the grace period.

!!! warning "A worker that fails does not restart"
    If a scanner or cleanup worker returns an error — most commonly because Temporal is
    unreachable at startup — it logs `worker exited with error` and that thread is **gone for
    the lifetime of the process**. The HTTP server keeps serving and `/healthz` keeps
    returning `200`, so nothing external notices that scanning has stopped. Alert on the
    scanner metrics rather than on liveness; see [Metrics](metrics.md).

## In a container

The image sets an entrypoint and a default command, so it runs with no arguments:

```text
workdup --config /app/workdup.toml run
```
