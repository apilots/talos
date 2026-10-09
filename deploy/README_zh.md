# Talos 部署

[English](README.md)

> Historical translation. The [English document](README.md) owns the current
> 2026-09-07 contracts, diagrams, deployment rules and verification guidance.

本目录负责 `talos-server`、独立 `talos-worker` 部署示例以及已发布 `talos` 终端 bundle 的安装资源。

## 目录

```text
deploy/
|-- caddy/          Caddy reverse-proxy example
|-- docker/         Docker Compose deployment
|-- install/        TUI release installer and documentation
`-- nginx/          Nginx reverse-proxy example
```

## 服务端部署

`talos-server` 暴露 HTTP API、Web UI 和 SSE Timeline stream。反向代理必须对
`/v1/sessions/*/events` 禁用响应缓冲和压缩，确保事件增量送达。

Server 镜像同时包含 `talos-worker`。Worker 部署将容器命令覆盖为
`/opt/talos/bin/talos-worker`，并提供节点注册和 TLS 配置。

`caddy/`、`nginx/` 和 `docker/` 下的示例是维护中的起点。投入生产前需要检查认证、
TLS、持久化、资源限制和 secret 管理。

在 `.env` 中设置 Provider key 后，启动 Docker Compose 示例：

```bash
cd deploy/docker
cp .env.example .env
docker compose up -d
curl http://127.0.0.1:8080/health
```

## 终端安装

Release bundle installer 位于 [`install/`](install/README.md)，它将产品资源与
用户拥有的配置和 Runtime 状态分开安装。

部署变更必须验证真实启动或安装路径，不能只做静态配置检查。
