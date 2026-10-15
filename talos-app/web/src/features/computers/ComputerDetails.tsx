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
    {rows.filter(([, value]) => value !== undefined && value !== null).map(([label, value]) =>
      <div key={label} className="flex flex-wrap justify-between gap-x-4 gap-y-1 py-3">
        <dt className="shrink-0 text-muted">{t(label)}</dt>
        <dd className={`min-w-0 max-w-full break-words text-right ${label === "nodeId" ? "font-mono text-xs" : ""}`}>{value}</dd>
      </div>)}
  </dl>
    {Boolean(info?.networkInterfaces.length) && <details className="border-t border-border py-3">
      <summary className="cursor-pointer text-muted">{t("networkCount", { count: info?.networkInterfaces.length })}</summary>
      <dl className="mt-2 space-y-2">
        {info?.networkInterfaces.map((network) => <div key={network.name} className="flex justify-between gap-4">
          <dt className="text-muted">{network.name}</dt>
          <dd className="font-mono text-xs">{network.macAddress}</dd>
        </div>)}
      </dl>
    </details>}
  </div>;
}
