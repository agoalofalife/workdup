# Overview

What workdup is, the replay-cost problem it solves, and the idea in one diagram.

workdup watches your Temporal namespaces and maintains a live, deduplicated set of workflows — one representative per distinct *shape* of execution. Point your replay tests at that set instead of at every history and you cover the same code paths with a fraction of the downloads.

## The problem

Replay testing verifies that today's workflow code can still deterministically replay yesterday's histories. Doing it properly means fetching histories, and that gets expensive for three reasons:

- **Volume.** Downloading every history in a busy namespace moves hundreds of megabytes and takes tens of minutes.
- **Duplication.** Most of those histories are the same execution path with different inputs and timestamps. Replaying a thousand copies of one path finds nothing a single copy wouldn't.
- **Staleness.** A full export is out of date the moment it finishes. Workflows start, advance and complete continuously.

## The idea

Reduce each history to a **sequence of canonical tokens** — the decisions the workflow code made, with the per-run noise stripped out — and hash that. Two workflows that took the same path through your code produce the same hash, however different their payloads and timestamps.

```
flowchart LR
    A[Temporal namespace] -->|Visibility API| B[Scanner]
    B -->|changed only| C[Fetch history]
    C --> D[Normalize to tokens]
    D --> E[SHA-256]
    E --> F[(SQLite)]
    F --> G["GET /unique-workflows"]
    G --> H[Your replay suite]
```

The scanner only downloads a history when Temporal reports that the workflow's `HistoryLength` has changed since the last look, so a steady-state namespace costs one cheap list call per interval and nothing else.

## What you get

A single binary that runs as a daemon, and an HTTP endpoint that answers "which workflows should I replay?":

```console
$ curl 'localhost:8000/unique-workflows?namespace=orders-prod'
[{"workflow_id":"order-4821","run_id":"0198f2c1-...","workflow_type":"OrderFulfillment"}]
```

Your test suite fetches those histories with its own Temporal SDK and replays them. workdup never runs your tests and never needs to know what language they are written in — see [How to use](https://agoalofalife.github.io/workdup/dev/usage/index.md).

## Terminology

| Term                | Meaning                                                                       |
| ------------------- | ----------------------------------------------------------------------------- |
| **Workflow**        | One execution in Temporal: an ordered sequence of history events              |
| **Replay**          | Re-running a recorded history against current code to check determinism       |
| **Semantic hash**   | SHA-256 of a workflow's normalized token sequence                             |
| **Canonical token** | A short string standing for one meaningful event, e.g. `A:payments.charge`    |
| **Visibility API**  | Temporal's metadata query API — lists workflows without downloading histories |
| **Timer bucket**    | The rounded duration a timer's timeout is reduced to before hashing           |

## Where to go next

| If you want to               | Read                                                                                 |
| ---------------------------- | ------------------------------------------------------------------------------------ |
| Install it                   | [Installation](https://agoalofalife.github.io/workdup/dev/installation/index.md)     |
| Wire it into CI              | [How to use](https://agoalofalife.github.io/workdup/dev/usage/index.md)              |
| Understand the hashing       | [How it works](https://agoalofalife.github.io/workdup/dev/how-it-works/index.md)     |
| Configure namespaces and TLS | [Configuration](https://agoalofalife.github.io/workdup/dev/configuration/index.md)   |
| Query it                     | [HTTP endpoints](https://agoalofalife.github.io/workdup/dev/http-endpoints/index.md) |
| Monitor it                   | [Metrics](https://agoalofalife.github.io/workdup/dev/metrics/index.md)               |
