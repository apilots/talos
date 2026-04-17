import { useEffect, useMemo, useState } from "react";
import {
  Activity,
  AlertTriangle,
  Bot,
  Cpu,
  Gauge,
  ListChecks,
  ShieldCheck,
  ShieldPlus,
  Users,
} from "lucide-react";

import { useModelsQuery, useSessionsQuery } from "@/features/chat/hooks/useChatQueries";
import { formatRelativeTime } from "@/shared/lib/time";
import { useAppStore } from "@/shared/store/app-store";
import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

type AdminSection = "dashboard" | "users" | "sessions" | "tools" | "audit" | "models" | "system";

const sections: Array<{ id: AdminSection; label: string; icon: typeof Activity }> = [
  { id: "dashboard", label: "Dashboard", icon: Activity },
  { id: "users", label: "Users", icon: Users },
  { id: "sessions", label: "Sessions", icon: ListChecks },
  { id: "tools", label: "Tools & Perms", icon: ShieldPlus },
  { id: "audit", label: "Audit Log", icon: ShieldCheck },
  { id: "models", label: "Models", icon: Bot },
  { id: "system", label: "System", icon: Cpu },
];

const userRows = [
  { id: "u-1", name: "xujiang", role: "Admin", status: "Active", lastSeen: "5 min ago" },
  { id: "u-2", name: "alice", role: "Member", status: "Active", lastSeen: "2 hours ago" },
  { id: "u-3", name: "bob", role: "Member", status: "Suspended", lastSeen: "3 days ago" },
];

const auditRows = [
  { id: "a-1", time: "14:32:11", actor: "xujiang", event: "exec_command", result: "Approved", detail: "cargo build --release" },
  { id: "a-2", time: "14:31:58", actor: "xujiang", event: "write_file", result: "Rejected", detail: "/etc/nginx/default.conf" },
  { id: "a-3", time: "14:20:03", actor: "alice", event: "search_web", result: "Auto-allowed", detail: "rust async runtime" },
];

const toolRows = [
  { id: "t-1", name: "Web search", policy: "Allowed", detail: "Safe read-only access to network lookups." },
  { id: "t-2", name: "Read local files", policy: "Allowed", detail: "Open files from the configured server-visible workspace." },
  { id: "t-3", name: "Write files", policy: "Needs approval", detail: "Mutating operations stay gated behind explicit approval." },
  { id: "t-4", name: "Terminal commands", policy: "Needs approval", detail: "Command execution requires a user confirmation step." },
  { id: "t-5", name: "External HTTP", policy: "Allowed", detail: "Remote requests follow the server governance policy." },
];

export function AdminPage() {
  const sessions = useSessionsQuery();
  const models = useModelsQuery();
  const connection = useAppStore((state) => state.connection);
  const metrics = useAppStore((state) => state.adminMetrics);
  const setAdminMetrics = useAppStore((state) => state.setAdminMetrics);
  const [activeSection, setActiveSection] = useState<AdminSection>("dashboard");
  const [sessionSearch, setSessionSearch] = useState("");

  const sessionRows = useMemo(() => {
    const query = sessionSearch.trim().toLowerCase();
    return (sessions.data ?? []).filter((session) => {
      if (!query) {
        return true;
      }
      return [session.title, session.config.llm?.model ?? "", session.context.session_id]
        .join(" ")
        .toLowerCase()
        .includes(query);
    });
  }, [sessionSearch, sessions.data]);

  useEffect(() => {
    setAdminMetrics([
      {
        id: "active-sessions",
        label: "Active sessions",
        value: String(sessions.data?.length ?? 0),
        trend: "Visible through current shared session list",
      },
      {
        id: "model-endpoints",
        label: "Configured models",
        value: String(models.data?.length ?? 0),
        trend: "Discovered from /v1/models",
      },
      {
        id: "pending-approvals",
        label: "Pending approvals",
        value: "2",
        trend: "Derived from current design scenarios",
      },
      {
        id: "operational-risk",
        label: "Operational risk",
        value: "Low",
        trend: "No backend mutations in this Web UI pass",
      },
    ]);
  }, [models.data?.length, sessions.data?.length, setAdminMetrics]);

  return (
    <div className="mx-auto flex h-full w-full max-w-[1240px] flex-col gap-5 overflow-y-auto px-8 py-7">
      <Panel className="rounded-[1.5rem] border-border/80 bg-card/88 p-6 backdrop-blur">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="text-xs uppercase tracking-[0.22em] text-muted">Admin</div>
            <h1 className="mt-2 font-display text-[30px] tracking-[-0.05em] text-ink">Operational control room</h1>
            <p className="mt-3 max-w-2xl text-sm leading-6 text-muted">
              Visualize shared sessions, governance, and system health from a single place. These views stay
              read-heavy until dedicated admin APIs are added.
            </p>
          </div>
          <div className="flex items-center gap-3">
            <div className="text-xs text-muted">Today · 2026-04-16</div>
            <Badge tone="warning">Admin preview</Badge>
          </div>
        </div>
      </Panel>

      <div className="grid gap-5 xl:grid-cols-[220px_minmax(0,1fr)]">
        <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-3">
          <div className="mb-3 px-3 pt-2 text-xs uppercase tracking-[0.18em] text-muted">Talos Admin</div>
          <nav className="space-y-1">
            {sections.map((section) => {
              const Icon = section.icon;
              return (
                <button
                  className={[
                    "flex w-full items-center gap-3 rounded-2xl px-4 py-3 text-left text-sm transition",
                    activeSection === section.id
                      ? "bg-background/70 text-ink shadow-panel"
                      : "text-muted hover:bg-background/45 hover:text-ink",
                  ].join(" ")}
                  key={section.id}
                  onClick={() => setActiveSection(section.id)}
                  type="button"
                >
                  <Icon size={16} />
                  <span>{section.label}</span>
                </button>
              );
            })}
          </nav>
        </Panel>

        <div className="space-y-5">
          {activeSection === "dashboard" ? (
            <>
              <div className="grid gap-4 xl:grid-cols-4">
                {metrics.map((metric, index) => {
                  const Icon = [Activity, Gauge, AlertTriangle, Cpu][index % 4];
                  return (
                    <Panel className="rounded-[1.35rem] border-border/80 bg-card/82 p-5" key={metric.id}>
                      <div className="flex items-center justify-between gap-3">
                        <div className="text-xs uppercase tracking-[0.18em] text-muted">{metric.label}</div>
                        <Icon className="text-brand" size={16} />
                      </div>
                      <div className="mt-4 font-display text-4xl tracking-[-0.05em] text-ink">{metric.value}</div>
                      <p className="mt-3 text-sm text-muted">{metric.trend}</p>
                    </Panel>
                  );
                })}
              </div>
              <div className="grid gap-5 xl:grid-cols-[1.2fr_0.9fr]">
                <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
                  <div className="text-sm font-medium text-ink">Active agent tasks</div>
                  <div className="mt-4 space-y-3">
                    {(sessions.data ?? []).slice(0, 3).map((session) => (
                      <div className="rounded-[1.15rem] border border-border/80 bg-background/45 p-4" key={session.context.session_id}>
                        <div className="flex items-center justify-between gap-3">
                          <div className="text-sm font-medium text-ink">{session.title}</div>
                          <Badge tone="success">Running</Badge>
                        </div>
                        <div className="mt-2 text-xs text-muted">
                          User {connection.userId} · {formatRelativeTime(session.state.last_activity)}
                        </div>
                      </div>
                    ))}
                  </div>
                </Panel>
                <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
                  <div className="text-sm font-medium text-ink">Top tools</div>
                  <div className="mt-4 space-y-4">
                    {[
                      ["read_file", 1203],
                      ["exec_command", 421],
                      ["search_web", 389],
                      ["write_file", 201],
                      ["http_request", 98],
                    ].map(([label, value]) => (
                      <div key={label}>
                        <div className="mb-2 flex items-center justify-between text-sm">
                          <span className="text-ink">{label}</span>
                          <span className="text-muted">{value}</span>
                        </div>
                        <div className="h-2 rounded-full bg-background/70">
                          <div
                            className="h-2 rounded-full bg-brand/75"
                            style={{ width: `${(Number(value) / 1203) * 100}%` }}
                          />
                        </div>
                      </div>
                    ))}
                  </div>
                </Panel>
              </div>
            </>
          ) : null}

          {activeSection === "users" ? (
            <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
              <div className="mb-4 flex items-center justify-between gap-3">
                <div>
                  <div className="text-sm font-medium text-ink">Users</div>
                  <div className="mt-1 text-sm text-muted">Manage roles, activation state, and local access previews.</div>
                </div>
                <Button variant="secondary">Create user</Button>
              </div>
              <div className="space-y-3">
                {userRows.map((user) => (
                  <div className="grid grid-cols-[minmax(0,1fr)_140px_140px_140px_120px] items-center gap-4 rounded-[1.2rem] border border-border/80 bg-background/45 px-4 py-4" key={user.id}>
                    <div className="font-medium text-ink">{user.name}</div>
                    <div className="text-sm text-muted">{user.role}</div>
                    <div className="text-sm text-muted">{user.status}</div>
                    <div className="text-sm text-muted">{user.lastSeen}</div>
                    <div className="flex justify-end">
                      <Button variant="ghost">Edit</Button>
                    </div>
                  </div>
                ))}
              </div>
            </Panel>
          ) : null}

          {activeSection === "sessions" ? (
            <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
              <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
                <div>
                  <div className="text-sm font-medium text-ink">Sessions</div>
                  <div className="mt-1 text-sm text-muted">Inspect running, blocked, and completed conversations.</div>
                </div>
                <Input
                  className="max-w-xs"
                  onChange={(event) => setSessionSearch(event.target.value)}
                  placeholder="Search sessions..."
                  value={sessionSearch}
                />
              </div>
              <div className="space-y-3">
                {sessionRows.map((session) => (
                  <div className="grid grid-cols-[84px_minmax(0,1fr)_120px_110px_120px] items-center gap-4 rounded-[1.2rem] border border-border/80 bg-background/45 px-4 py-4" key={session.context.session_id}>
                    <Badge tone="success">Running</Badge>
                    <div className="min-w-0">
                      <div className="truncate text-sm font-medium text-ink">{session.title}</div>
                      <div className="mt-1 truncate text-xs text-muted">{session.context.session_id}</div>
                    </div>
                    <div className="text-sm text-muted">{connection.userId}</div>
                    <div className="text-sm text-muted">{formatRelativeTime(session.state.last_activity)}</div>
                    <div className="flex justify-end gap-2">
                      <Button variant="ghost">View</Button>
                      <Button variant="danger">Stop</Button>
                    </div>
                  </div>
                ))}
              </div>
            </Panel>
          ) : null}

          {activeSection === "tools" ? (
            <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
              <div className="mb-4 flex items-center justify-between gap-3">
                <div>
                  <div className="text-sm font-medium text-ink">Tools and permissions</div>
                  <div className="mt-1 text-sm text-muted">Review the effective posture of common tools and approval requirements.</div>
                </div>
                <Button variant="secondary">Edit global policy</Button>
              </div>
              <div className="space-y-3">
                {toolRows.map((tool) => (
                  <div
                    className="grid grid-cols-[minmax(0,1fr)_180px] gap-4 rounded-[1.2rem] border border-border/80 bg-background/45 px-4 py-4"
                    key={tool.id}
                  >
                    <div>
                      <div className="text-sm font-medium text-ink">{tool.name}</div>
                      <div className="mt-1 text-sm text-muted">{tool.detail}</div>
                    </div>
                    <div className="flex items-center justify-end">
                      <Badge tone={tool.policy === "Allowed" ? "success" : "warning"}>{tool.policy}</Badge>
                    </div>
                  </div>
                ))}
              </div>
            </Panel>
          ) : null}

          {activeSection === "audit" ? (
            <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
              <div className="mb-4 flex items-center justify-between gap-3">
                <div>
                  <div className="text-sm font-medium text-ink">Audit log</div>
                  <div className="mt-1 text-sm text-muted">Trace approvals, denials, and autonomous tool execution.</div>
                </div>
                <Button variant="secondary">Export CSV</Button>
              </div>
              <div className="space-y-3">
                {auditRows.map((entry) => (
                  <div className="rounded-[1.2rem] border border-border/80 bg-background/45 p-4" key={entry.id}>
                    <div className="flex flex-wrap items-center justify-between gap-3">
                      <div className="flex items-center gap-3 text-sm">
                        <span className="text-muted">{entry.time}</span>
                        <span className="font-medium text-ink">{entry.actor}</span>
                        <span className="text-ink">{entry.event}</span>
                      </div>
                      <Badge tone={entry.result === "Rejected" ? "danger" : "success"}>{entry.result}</Badge>
                    </div>
                    <div className="mt-2 text-sm text-muted">{entry.detail}</div>
                  </div>
                ))}
              </div>
            </Panel>
          ) : null}

          {activeSection === "models" ? (
            <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
              <div className="mb-4 flex items-center justify-between gap-3">
                <div>
                  <div className="text-sm font-medium text-ink">Models</div>
                  <div className="mt-1 text-sm text-muted">Status snapshots for model providers visible to the current server.</div>
                </div>
                <Button variant="secondary">Add model</Button>
              </div>
              <div className="space-y-3">
                {(models.data ?? []).map((model) => (
                  <div className="rounded-[1.2rem] border border-border/80 bg-background/45 p-4" key={model.id}>
                    <div className="flex items-center justify-between gap-3">
                      <div>
                        <div className="font-medium text-ink">{model.name}</div>
                        <div className="mt-1 text-xs text-muted">
                          Provider {model.provider} · Context {model.context_window ?? 0}
                        </div>
                      </div>
                      <Badge tone="success">Healthy</Badge>
                    </div>
                  </div>
                ))}
              </div>
            </Panel>
          ) : null}

          {activeSection === "system" ? (
            <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
              <div className="mb-5 text-sm font-medium text-ink">System status</div>
              <div className="grid gap-4 md:grid-cols-2">
                <div className="rounded-[1.2rem] border border-border/80 bg-background/45 p-4">
                  <div className="text-xs uppercase tracking-[0.18em] text-muted">Server</div>
                  <div className="mt-3 space-y-2 text-sm text-muted">
                    <div>Version talos-server v0.3.1</div>
                    <div>Address {new URL(connection.serverUrl).host}</div>
                    <div>Uptime 2d 5h</div>
                  </div>
                </div>
                <div className="rounded-[1.2rem] border border-border/80 bg-background/45 p-4">
                  <div className="text-xs uppercase tracking-[0.18em] text-muted">Clients</div>
                  <div className="mt-3 space-y-2 text-sm text-muted">
                    <div>Web console · 127.0.0.1 · Connected</div>
                    <div>TUI · 127.0.0.1 · Connected</div>
                  </div>
                </div>
              </div>
              <div className="mt-5 flex flex-wrap items-center gap-3">
                <Button variant="secondary">Reload config</Button>
                <Button variant="secondary">Clean expired sessions</Button>
                <Button variant="danger">Restart server</Button>
              </div>
            </Panel>
          ) : null}
        </div>
      </div>
    </div>
  );
}
