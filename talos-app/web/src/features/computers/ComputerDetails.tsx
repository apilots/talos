import type { AppProtocol } from "@talos/sdk-core";
import { useTranslation } from "react-i18next";

/** Read-only Computer metadata. Management actions remain owned by the page. */
export function ComputerDetails({ node, controlPlane }: { node: AppProtocol.NodeInfo; controlPlane: string }) {
  const { t, i18n } = useTranslation("computers");
  const info = node.systemInfo;
  const timestamp = (value: number | null) => value ? new Date(value * 1_000).toLocaleString(i18n.language) : undefined;
  const rows: [string, string | number | undefined | null][] = [
    ["system", info ? (info.osVersion ?? (info.os === "macOs" ? "macOS" : "Linux")) : undefined],
    ["architecture", info?.architecture], ["hostname", info?.hostname], ["processor", info?.cpuModel],
    ["cpuCount", info?.logicalCpuCount || undefined],
    ["memory", info?.memoryBytes ? `${(info.memoryBytes / 1024 ** 3).toLocaleString(i18n.language, { maximumFractionDigits: 1 })} GiB` : undefined],
    ["board", info?.boardModel], ["graphics", info?.graphics.join(", ") || undefined],
    ["workerVersion", info?.workerVersion], ["heartbeat", timestamp(node.lastHeartbeatAt)],
    ["created", timestamp(node.createdAt)], ["updated", timestamp(node.updatedAt)],
    ["owner", node.ownerUserId], ["controlPlane", controlPlane], ["nodeId", node.nodeId],
  ];
  return <div className="text-sm"><dl className="divide-y divide-border">
    {rows.map(([label, value]) =>
      <div key={label} className="grid grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)] items-start gap-x-4 gap-y-1 py-2.5 text-xs leading-5">
        <dt className="min-w-0 text-muted">{t(label)}</dt>
        <dd className={`min-w-0 [overflow-wrap:anywhere] text-right font-medium text-ink ${label === "nodeId" ? "font-mono text-xs" : ""}`}>{value === undefined || value === null || value === "" ? "N/A" : value}</dd>
      </div>)}
  </dl>
    <details className="border-t border-border py-3">
      <summary className="cursor-pointer text-muted">{info ? t("networkCount", { count: info.networkInterfaces.length }) : `${t("network")} · N/A`}</summary>
      <dl className="mt-2 space-y-2">
        {!info?.networkInterfaces.length ? <div><dt>{t("network")}</dt><dd>N/A</dd></div> : null}
        {info?.networkInterfaces.map((network) => <div key={network.name} className="grid grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)] gap-4 text-xs leading-5">
          <dt className="text-muted">{network.name}</dt>
          <dd className="min-w-0 [overflow-wrap:anywhere] text-right font-mono text-xs">{network.macAddress}</dd>
        </div>)}
      </dl>
    </details>
  </div>;
}
