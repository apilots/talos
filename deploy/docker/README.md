# Docker deployment examples

| Product | Examples |
| --- | --- |
| [talos-server](talos-server/README.md) | Basic Web/API deployment and authenticated production registration/RPC deployment |
| [talos-worker](talos-worker/README.md) | Independent execution process with an owned Computer credential and outbound Connect |

Each product directory owns its Compose files, deployment `.env.example` and
runtime YAML example. Desktop and TUI are user clients, not additional Docker
services in these examples. Deploy each product separately; there is no combined
stack that starts Workers as part of a Server rollout.
