# Docker deployment examples

| Product | Examples |
| --- | --- |
| [talos-server](talos-server/README.md) | Basic Web/API, authenticated production, and optional Docker Engine overlay for Talo |
| [talos-worker](talos-worker/README.md) | Independent execution process with an owned Computer credential and outbound Connect |

Each product directory owns its Compose files, deployment `.env.example` and
runtime YAML example. Desktop and TUI are user clients, not additional Docker
services in these examples. Deploy a manual Worker separately. Managed Talo is
created by Server at runtime through Docker Engine. See the [Talos Cloud
deployment guide](../TALOS_CLOUD_DEPLOY.md) for Runtime and network boundaries.
