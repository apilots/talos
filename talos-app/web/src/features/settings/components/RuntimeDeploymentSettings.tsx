import type {
  WorkerKubernetesSettings,
  WorkerRuntimeBackend,
  WorkerRuntimeProfile,
  WorkerRuntimeSettingsResponse,
} from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";

import { Input } from "@/shared/ui/Input";

export function RuntimeDeploymentSettings({
  backend,
  dockerSocketPath,
  kubernetes,
  profile,
  timing,
  idleTimeout,
  onIdleTimeoutChange,
}: {
  backend: WorkerRuntimeBackend;
  dockerSocketPath: string;
  kubernetes: WorkerKubernetesSettings;
  profile: WorkerRuntimeProfile;
  timing: NonNullable<WorkerRuntimeSettingsResponse["timing"]>;
  idleTimeout: string | undefined;
  onIdleTimeoutChange: (value: string) => void;
}) {
  const { t, i18n } = useTranslation("settings", { keyPrefix: "runtimePanel" });
  const number = new Intl.NumberFormat(i18n.resolvedLanguage);
  const gib = (bytes: number) => t("gib", { value: number.format(bytes / 1024 ** 3) });
  const seconds = (value: number) => t("seconds", { value: number.format(value) });
  const home = backend === "dockerEngine" ? t("unlimitedHome") :
    t("homeCapacity", { value: number.format(kubernetes.homeBytes / 1_000_000_000) });

  return (
    <>
      <div className="space-y-3 border-t border-divider pt-4">
        <h3 className="text-sm font-medium text-ink">{t("serverPolicy")}</h3>
        <label className="flex flex-wrap items-center gap-3 text-sm text-muted">
          <span>{t("idleTimeout")}</span>
          <Input className="max-w-40" min={1} max={4294967295} required type="number" value={idleTimeout ?? timing.idleTimeoutSeconds} onChange={(event) => onIdleTimeoutChange(event.target.value)} />
        </label>
      </div>
      <div className="space-y-3 border-t border-divider pt-4">
        <div className="space-y-1">
          <h3 className="text-sm font-medium text-ink">{t("deploymentDefaults")}</h3>
          <p className="text-xs leading-5 text-muted">{t("deploymentDefaultsHelp")}</p>
        </div>
        {backend === "dockerEngine" ? (
          <label className="block space-y-2 text-sm text-muted">
            <span>{t("dockerSocket")}</span>
            <Input className="font-mono" readOnly value={dockerSocketPath} />
          </label>
        ) : null}
        <dl className="grid gap-x-5 gap-y-3 text-sm sm:grid-cols-2">
          <RuntimeValue label={t("cpu")} value={number.format(profile.cpuCores)} />
          <RuntimeValue label={t("memory")} value={gib(profile.memoryBytes)} />
          <RuntimeValue label={t("sharedMemory")} value={gib(profile.shmBytes)} />
          <RuntimeValue label={t("home")} value={home} />
          <RuntimeValue label={t("requestWait")} value={seconds(timing.requestWaitSeconds)} />
          <RuntimeValue label={t("startTimeout")} value={seconds(timing.startTimeoutSeconds)} />
          <RuntimeValue label={t("wakeCooldown")} value={seconds(timing.wakeCooldownSeconds)} />
          {backend === "kubernetes" ? (
            <>
              <RuntimeValue label={t("seccomp")} value={kubernetes.seccompProfile} />
              <RuntimeValue label={t("homeQuota")} value={t(kubernetes.homeQuotaEnforced ? "quotaEnforced" : "quotaUnverified")} />
            </>
          ) : null}
        </dl>
        {backend === "kubernetes" ? <p className="text-xs leading-5 text-muted">{t("localVolumeAffinity")}</p> : null}
      </div>
    </>
  );
}

function RuntimeValue({ label, value }: { label: string; value: string }) {
  return (
    <div className="space-y-1">
      <dt className="text-muted">{label}</dt>
      <dd className="break-all font-medium text-ink">{value}</dd>
    </div>
  );
}
