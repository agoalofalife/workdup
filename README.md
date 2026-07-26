# workdup

Temporal workflow history deduplication for replay-test optimization — maintains a live, deduplicated set of "unique" workflows (by semantic hash) so CI/QA replays a minimal covering set instead of every history.

## Documentation

Full documentation lives at **<https://agoalofalife.github.io/workdup/>** — versioned and searchable, built from [`docs/`](docs/) in this repo. The root URL resolves to the current default version.

The Grafana dashboard manual that used to fill this file is now [`docs/metrics.md`](docs/metrics.md): every metric workdup emits, the exact PromQL behind each panel, how to read it, and the alert worth setting.

## License

[MIT](LICENSE)
