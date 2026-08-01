# Environment variables

Every variable the binary reads, plus the installer-only ones.

## Runtime

| Variable | Read by | Default | Purpose |
| --- | --- | --- | --- |
| `WORKDUP_CONFIG` | CLI | `workdup.toml` | Config file path. `--config` overrides it |
| `RUST_LOG` | Logging | `info` | Tracing filter. Invalid syntax falls back to `info` |
| `WORKDUP__<SECTION>__<KEY>` | Config | — | Overrides one config value, e.g. `WORKDUP__HTTP__PORT=8080` |
| Anything named in the config | Config | — | Every `${VAR}` in `workdup.toml` must be set, or startup fails |

### One underscore or two

| Form | Meaning |
| --- | --- |
| `WORKDUP_CONFIG` | Single underscore. A CLI flag fallback — which file to read |
| `WORKDUP__HTTP__PORT` | Double underscore. A config overlay — what a value inside the file becomes |

They are separate mechanisms. `WORKDUP__CONFIG` is not a valid override and is rejected as an
unknown field.

### `RUST_LOG`

Standard `tracing` filter syntax:

```bash
RUST_LOG=info                              # everything at info and above
RUST_LOG=warn,workdup=debug                # debug for the app, warn for dependencies
RUST_LOG=workdup::scanner=trace            # one module
```

Output is JSON on stdout. See [Logs](logs.md).

## `.env`

If a `.env` file exists in the working directory it is loaded at startup, before logging is
initialised and before the config is read. Variables already present in the real environment
are **not** overwritten, so `.env` acts as a default layer:

```ini title=".env"
RUST_LOG=info
TEMPORAL_TLS_CLIENT_CERT_PATH=/secrets/tls.crt
TEMPORAL_TLS_CLIENT_KEY_PATH=/secrets/tls.key
TENANT_A_API_KEY=...
```

`.env` and `*.pem` are gitignored in this repository. Keep them out of images and ConfigMaps —
they are the secret half of the split described in [Configuration](configuration.md).

## Installer

Read by [`install.sh`](installation.md#install-script) only. The binary never sees them.

| Variable | Default | Purpose |
| --- | --- | --- |
| `WORKDUP_VERSION` | `latest` | Tag to install, e.g. `v0.2.0` |
| `WORKDUP_INSTALL_DIR` | `/usr/local/bin` | Destination directory |
| `WORKDUP_REPO` | `agoalofalife/workdup` | Source repository, for forks and mirrors |
