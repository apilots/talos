# Shared container runtime

Server and Worker images use `prepare-browser.sh` and `install-runtime.sh` with
the same Ubuntu and browser supplier inputs. A separate stage snapshots the immutable
official MCP source selected by `build/plugins/official-mcp.json`, without
invalidating the Chromium download stage when that source changes. The runtime
installs Browser's exact wheel hashes from that snapshot and generates its
environment metadata. Update that source rather than adding a container lockfile.

Both images include SSH/SCP/SFTP clients, curl/wget, DNS tools, ip/ss,
ping/traceroute/nc, ps/top/pgrep, lsof, jq, less/vi, rsync, compression tools and
OpenSSL. These are local clients under the existing non-root execution and
sandbox policies. Credentials and remote access configuration remain operator
inputs. There is no SSH server or additional daemon. kubectl, cron, tcpdump and
strace remain outside the common runtime; deployment-specific tooling and
privileges require an explicitly maintained image variant.

The Server image additionally takes `talos` from the same run's validated TUI
product. The default entrypoint starts only Server; the Worker image remains a
standalone execution process without Server, WebUI or TUI. Local source image
builds compile their own selected products; release images use
`PRODUCT_STAGE=packaged` and never compile replacement product binaries.

Use the existing Chromium seccomp profile, `no-new-privileges` and 1 GiB private
shared memory for either image. Browser resources start lazily. Memory budgets
must account for the Server, embedded Worker and actual browser workload; the
Server Compose profiles do not impose a universal 1 GiB process memory limit.

The Server's embedded Docker Worker uses the existing desktop media relay through
an in-process adapter. Viewing and control follow the deployment's existing
authentication mode, including anonymous local access when `gateway.auth=false`.
Registered Worker media retains its user-token and node-lease admission. Neither
path publishes VNC/CDP ports or bypasses the Browser owner's input/control fencing.
