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
|-- kubernetes/     Kubernetes Server and Worker deployment
`-- nginx/          Nginx reverse-proxy example
```

## 服务端部署

[云部署手册](TALOS_CLOUD_DEPLOY.md)汇总 Docker Engine 和 Kubernetes 的选择、部署入口和网络边界，也说明本地回环访问和生产域名入口。

`talos-server` 暴露 HTTP API、Web UI 和 SSE Timeline stream。反向代理必须对
`/v1/sessions/*/events` 禁用响应缓冲和压缩，确保事件增量送达。

独立 Worker 使用专用 `ghcr.io/apilots/talos-worker` 镜像，通过 outbound
Connect 注册，不继承 Server 的 HTTP 健康检查。持久目录、节点 UUID/凭据
和 TLS 信任配置见 [Worker 镜像说明](../build/worker/README.md)。Talos Release 会发布
`linux/amd64` 和 `linux/arm64` 的 Server、Worker 镜像。

`caddy/`、`nginx/` 和 `docker/` 下的示例是维护中的起点。投入生产前需要检查认证、
TLS、持久化、资源限制和 secret 管理。

在 `.env` 中设置 Provider key 后，启动 Docker Compose 示例：

```bash
cd deploy/docker/talos-server
cp .env.example .env
docker compose up -d
curl http://127.0.0.1:8080/health
```

`agent.talos.love` 的生产配置、TLS 入口、升级回滚及验收见 [Docker 生产部署](TALOS_CLOUD_DEPLOY.md#docker-production-on-talos-host)。普通 Compose 示例在回环地址提供 Web 和 HTTP API。

Kubernetes 下的 Server、手动注册 Worker、托管 Talo 及本地 k3d 验收见 [Kubernetes 部署手册](kubernetes/README.md)。

## 终端安装

Release bundle installer 位于 [`install/`](install/README.md)，它将产品资源与
用户拥有的配置和 Runtime 状态分开安装。

部署变更必须验证真实启动或安装路径，不能只做静态配置检查。

Docker 示例按产品分目录维护：[talos-server](docker/talos-server/README.md) 与 [talos-worker](docker/talos-worker/README.md)。Worker 独立部署，不随 Server 部署流程启动。
