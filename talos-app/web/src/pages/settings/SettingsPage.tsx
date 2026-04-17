import { useEffect, useMemo, useState, type ReactNode } from "react";
import { CheckCircle2, Globe2, RefreshCw, ServerCog, Sparkles } from "lucide-react";

import { useModelsQuery } from "@/features/chat/hooks/useChatQueries";
import { useAppStore } from "@/shared/store/app-store";
import { getApprovalModeHint, getApprovalModeLabel } from "@/shared/types/domain";
import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

type SettingsTab = "general" | "models" | "keys" | "appearance" | "server";

const tabs: Array<{ id: SettingsTab; label: string }> = [
  { id: "general", label: "General" },
  { id: "models", label: "Models" },
  { id: "keys", label: "API Keys" },
  { id: "appearance", label: "Appearance" },
  { id: "server", label: "Server" },
];

export function SettingsPage() {
  const connection = useAppStore((state) => state.connection);
  const setConnection = useAppStore((state) => state.setConnection);
  const uiTheme = useAppStore((state) => state.uiTheme);
  const setUiTheme = useAppStore((state) => state.setUiTheme);
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");
  const [draft, setDraft] = useState(connection);
  const [defaultModel, setDefaultModel] = useState<string>("");
  const [language, setLanguage] = useState("English");
  const [desktopNotifications, setDesktopNotifications] = useState(true);
  const [safeFileReads, setSafeFileReads] = useState(true);
  const [safeFileWrites, setSafeFileWrites] = useState(false);
  const [terminalApproval, setTerminalApproval] = useState(true);
  const models = useModelsQuery();

  const modelCards = useMemo(() => models.data ?? [], [models.data]);

  useEffect(() => {
    if (!defaultModel && modelCards[0]?.id) {
      setDefaultModel(modelCards[0].id);
    }
  }, [defaultModel, modelCards]);

  return (
    <div className="mx-auto flex h-full w-full max-w-[1120px] flex-col gap-5 overflow-y-auto px-8 py-7">
      <Panel className="rounded-[1.45rem] border-border/80 bg-card/88 p-5 backdrop-blur">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="text-xs uppercase tracking-[0.22em] text-muted">Settings</div>
            <h1 className="mt-2 font-display text-[28px] tracking-[-0.05em] text-ink">Configure the Web console</h1>
            <p className="mt-3 max-w-2xl text-sm leading-6 text-muted">
              Keep connection defaults, model preferences, and interface behavior aligned with the same
              shared API path the TUI already uses.
            </p>
          </div>
          <Badge tone={models.isSuccess ? "success" : "warning"}>
            {models.isSuccess ? "Connected" : "Awaiting server"}
          </Badge>
        </div>
      </Panel>

      <Panel className="rounded-[1.3rem] border-border/80 bg-card/82 p-2">
        <div className="flex flex-wrap gap-2">
          {tabs.map((tab) => (
            <button
              className={[
                "rounded-2xl px-4 py-2.5 text-sm transition",
                activeTab === tab.id
                  ? "bg-background/70 text-ink shadow-panel"
                  : "text-muted hover:bg-background/45 hover:text-ink",
              ].join(" ")}
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              type="button"
            >
              {tab.label}
            </button>
          ))}
        </div>
      </Panel>

      {activeTab === "general" ? (
        <div className="space-y-5">
          <div className="grid gap-4 md:grid-cols-3">
            <OverviewCard
              detail="Apply language changes immediately without a refresh."
              title="Language"
              value={language}
            />
            <OverviewCard
              detail="Current default approval behavior for new sessions."
              title="Approval mode"
              value={getApprovalModeLabel(draft.defaultApprovalMode)}
            />
            <OverviewCard
              detail="Switch between dark and light palettes in Appearance."
              title="Theme"
              value={uiTheme === "dark" ? "Black" : "White"}
            />
          </div>

          <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
            <SectionHeader
              icon={<Globe2 size={18} />}
              subtitle="Switch immediately and keep defaults predictable."
              title="Language and runtime"
            />
            <div className="mt-6 grid gap-5 md:grid-cols-2">
              <FieldBlock label="Language">
                <select
                  className="w-full rounded-2xl border border-border bg-surface/85 px-4 py-3 text-sm outline-none transition focus:border-brand/50"
                  onChange={(event) => setLanguage(event.target.value)}
                  value={language}
                >
                  <option>English</option>
                  <option>Chinese (Simplified)</option>
                </select>
                <p className="mt-2 text-xs text-muted">Supported: English / Chinese (Simplified)</p>
              </FieldBlock>
              <FieldBlock label="Runtime mode">
                <select
                  className="w-full rounded-2xl border border-border bg-surface/85 px-4 py-3 text-sm outline-none transition focus:border-brand/50"
                  onChange={(event) =>
                    setDraft((current) => ({
                      ...current,
                      runtimeMode: event.target.value as typeof current.runtimeMode,
                    }))
                  }
                  value={draft.runtimeMode}
                >
                  <option value="cloud">Cloud</option>
                  <option disabled value="local">
                    Local (desktop only)
                  </option>
                </select>
                <p className="mt-2 text-xs text-muted">Web currently stays on the shared cloud path.</p>
              </FieldBlock>
            </div>
          </Panel>

          <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
            <SectionHeader
              icon={<ServerCog size={18} />}
              subtitle="Keep safe defaults close to the conversation entry point."
              title="Default approvals"
            />
            <div className="mt-6 grid gap-4 md:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
              <FieldBlock label="Default approval policy">
                <select
                  className="w-full rounded-2xl border border-border bg-surface/85 px-4 py-3 text-sm outline-none transition focus:border-brand/50"
                  onChange={(event) =>
                    setDraft((current) => ({
                      ...current,
                      defaultApprovalMode: event.target.value as typeof current.defaultApprovalMode,
                    }))
                  }
                  value={draft.defaultApprovalMode}
                >
                  <option value="needs_approval">{getApprovalModeLabel("needs_approval")}</option>
                  <option value="skip">{getApprovalModeLabel("skip")}</option>
                </select>
              </FieldBlock>
              <div className="rounded-[1.2rem] border border-border/80 bg-background/45 p-4 text-sm text-muted">
                {getApprovalModeHint(draft.defaultApprovalMode)}
              </div>
            </div>
          </Panel>
        </div>
      ) : null}

      {activeTab === "models" ? (
        <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
          <div className="mb-5 flex items-center justify-between">
            <div>
              <div className="text-sm font-medium text-ink">Model configuration</div>
              <p className="mt-1 text-sm text-muted">Choose the default model and inspect available providers.</p>
            </div>
            <RefreshCw className={models.isFetching ? "animate-spin text-brand" : "text-muted"} size={16} />
          </div>
          <FieldBlock label="Default model">
            <select
              className="w-full rounded-2xl border border-border bg-surface/85 px-4 py-3 text-sm outline-none transition focus:border-brand/50"
              onChange={(event) => setDefaultModel(event.target.value)}
              value={defaultModel}
            >
              {modelCards.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.name}
                </option>
              ))}
            </select>
          </FieldBlock>
          <div className="mt-5 space-y-3">
            {modelCards.map((model) => (
              <div className="rounded-[1.25rem] border border-border/80 bg-background/45 p-4" key={model.id}>
                <div className="flex items-center justify-between gap-3">
                  <div className="font-medium text-ink">{model.name}</div>
                  <Badge>{model.provider}</Badge>
                </div>
                <div className="mt-2 text-xs text-muted">
                  Context {model.context_window ?? 0} · Max tokens {model.max_tokens ?? 0}
                </div>
              </div>
            ))}
          </div>
        </Panel>
      ) : null}

      {activeTab === "keys" ? (
        <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
          <SectionHeader
            icon={<CheckCircle2 size={18} />}
            subtitle="This pass keeps provider keys local to the browser until a dedicated secure path exists."
            title="API keys"
          />
          <div className="mt-5 space-y-4">
            {["OpenAI", "Anthropic", "Bailian"].map((provider) => (
              <div className="rounded-[1.25rem] border border-border/80 bg-background/45 p-4" key={provider}>
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <div className="font-medium text-ink">{provider}</div>
                    <div className="mt-1 text-xs text-muted">Stored locally for this browser session.</div>
                  </div>
                  <Badge tone="warning">Pending</Badge>
                </div>
                <Input className="mt-4" placeholder={`Paste ${provider} API key`} type="password" />
              </div>
            ))}
          </div>
        </Panel>
      ) : null}

      {activeTab === "appearance" ? (
        <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
          <SectionHeader
            icon={<Sparkles size={18} />}
            subtitle="Keep the UI dense, calm, and transcript-first."
            title="Appearance"
          />
          <div className="mt-6 grid gap-4 md:grid-cols-2">
              <FieldBlock label="Theme">
                <select
                  className="w-full rounded-2xl border border-border bg-surface/85 px-4 py-3 text-sm outline-none transition focus:border-brand/50"
                  onChange={(event) => setUiTheme(event.target.value as "dark" | "light")}
                  value={uiTheme}
                >
                  <option value="dark">Black</option>
                  <option value="light">White</option>
                </select>
              </FieldBlock>
            <button
              className={[
                "rounded-[1.2rem] border px-4 py-4 text-left text-sm transition",
                desktopNotifications
                  ? "border-brand/40 bg-brand/10 text-brand"
                  : "border-border bg-background/45 text-muted",
              ].join(" ")}
              onClick={() => setDesktopNotifications((current) => !current)}
              type="button"
            >
              <div className="font-medium">Desktop notifications</div>
              <div className="mt-2 text-xs leading-6">
                {desktopNotifications ? "Enabled for important state changes." : "Disabled in this browser."}
              </div>
            </button>
          </div>
        </Panel>
      ) : null}

      {activeTab === "server" ? (
        <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
          <SectionHeader
            icon={<ServerCog size={18} />}
            subtitle="Mirror the same host, identity, and permission defaults used by the shared SDK."
            title="Server connection"
          />
          <form
            className="mt-6 space-y-5"
            onSubmit={(event) => {
              event.preventDefault();
              setConnection(draft);
              void models.refetch();
            }}
          >
            <div className="grid gap-4 md:grid-cols-2">
              <FieldBlock label="Server URL">
                <Input
                  onChange={(event) => setDraft((current) => ({ ...current, serverUrl: event.target.value }))}
                  value={draft.serverUrl}
                />
              </FieldBlock>
              <FieldBlock label="User ID">
                <Input
                  onChange={(event) => setDraft((current) => ({ ...current, userId: event.target.value }))}
                  value={draft.userId}
                />
              </FieldBlock>
            </div>
            <FieldBlock label="Auth token">
              <Input
                autoComplete="current-password"
                onChange={(event) => setDraft((current) => ({ ...current, authToken: event.target.value }))}
                type="password"
                value={draft.authToken}
              />
            </FieldBlock>
            <div className="grid gap-4 md:grid-cols-3">
              <ToggleRow
                description="Permit safe read-only actions without extra prompts."
                label="Local file reads"
                onToggle={() => setSafeFileReads((current) => !current)}
                value={safeFileReads}
              />
              <ToggleRow
                description="Require explicit approval before write operations."
                label="File writes"
                onToggle={() => setSafeFileWrites((current) => !current)}
                value={safeFileWrites}
              />
              <ToggleRow
                description="Keep terminal execution gated behind a confirmation step."
                label="Terminal commands"
                onToggle={() => setTerminalApproval((current) => !current)}
                value={terminalApproval}
              />
            </div>
            <FieldBlock label="Detected server workspace">
              <Input disabled value={__TALOS_DEFAULT_PROJECT_ROOT__} />
            </FieldBlock>
            <div className="flex items-center justify-end gap-3">
              <Button
                onClick={() => {
                  setDraft(connection);
                  setSafeFileReads(true);
                  setSafeFileWrites(false);
                  setTerminalApproval(true);
                }}
                type="button"
                variant="ghost"
              >
                Reset
              </Button>
              <Button type="submit">
                <CheckCircle2 size={16} />
                Save
              </Button>
            </div>
          </form>
        </Panel>
      ) : null}
    </div>
  );
}

function OverviewCard({
  title,
  value,
  detail,
}: {
  title: string;
  value: string;
  detail: string;
}) {
  return (
    <Panel className="rounded-[1.2rem] border-border/80 bg-card/82 p-4">
      <div className="text-[10px] uppercase tracking-[0.18em] text-muted">{title}</div>
      <div className="mt-2 text-[1.65rem] font-display tracking-[-0.04em] text-ink">{value}</div>
      <p className="mt-1.5 text-sm leading-6 text-muted">{detail}</p>
    </Panel>
  );
}

function SectionHeader({
  icon,
  title,
  subtitle,
}: {
  icon: ReactNode;
  title: string;
  subtitle: string;
}) {
  return (
    <div className="flex items-start gap-4">
      <div className="flex size-11 items-center justify-center rounded-3xl border border-border bg-background/55 text-brand">
        {icon}
      </div>
      <div className="flex-1">
        <div className="text-sm font-medium text-ink">{title}</div>
        <p className="mt-1 text-sm text-muted">{subtitle}</p>
      </div>
    </div>
  );
}

function FieldBlock({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div>
      <label className="mb-2 block text-sm text-muted">{label}</label>
      {children}
    </div>
  );
}

function ToggleRow({
  label,
  description,
  value,
  onToggle,
}: {
  label: string;
  description: string;
  value: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      className="rounded-[1.25rem] border border-border/80 bg-background/45 p-4 text-left transition hover:border-brand/25"
      onClick={onToggle}
      type="button"
    >
      <div className="flex items-center justify-between gap-3">
        <div className="text-sm font-medium text-ink">{label}</div>
        <div className={value ? "text-brand" : "text-muted"}>{value ? "On" : "Off"}</div>
      </div>
      <p className="mt-2 text-xs leading-6 text-muted">{description}</p>
    </button>
  );
}
