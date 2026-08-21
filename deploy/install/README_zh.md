# Talos TUI installer

[English](README.md)

Installer 根据当前 macOS 或 Linux CPU target 选择 TUI release bundle，校验 release
checksum，安装不可变产品资源并启用稳定 launcher。

安装最新版本：

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/apilots/talos-releases/releases/latest/download/install.sh | sh
```

安装指定版本：

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/apilots/talos-releases/releases/latest/download/install.sh \
  | sh -s -- --version 0.1.7
```

默认路径：

```text
~/.local/share/talos/versions/<version>-<platform>-<arch>
~/.local/share/talos/current
~/.local/bin/talos
```

使用绝对路径 `TALOS_INSTALL_ROOT` 和 `TALOS_BIN_DIR` 覆盖安装位置。重复安装同一已验证
版本是幂等操作。Installer 不写入 `$TALOS_HOME` 或 `~/.talos`；配置升级、Runtime
migration、数据库、日志和 workspace 仍由 Talos 启动流程负责。

运行离线 installer 测试：

```bash
deploy/install/tests/install_test.sh
```

运行 native 和可用 Docker 验收路径：

```bash
just e2e-group 15_tui_install
```

Docker 只验证 Linux。macOS bundle 必须在原生 macOS 主机或 GitHub-hosted macOS runner 上运行。
