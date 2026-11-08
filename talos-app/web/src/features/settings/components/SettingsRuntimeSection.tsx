import { useQuery, useQueryClient } from "@tanstack/react-query";
import type {
  WorkerKubeconfigAuthentication,
  WorkerKubeconfigSource,
  WorkerKubernetesSettings,
  WorkerRuntimeBackend,
  WorkerRuntimeCandidate,
  WorkerRuntimeSettingsResponse,
} from "@talos/sdk-browser";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";
import { Input, Textarea } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import { workerRuntimeClient } from "../api";

const selectClass =
  "form-control-focus w-full rounded-[var(--radius-control)] border border-border bg-card px-4 py-3 text-sm text-ink";

export function SettingsRuntimeSection({ scope }: { scope: string }) {
  const { t } = useTranslation("settings", { keyPrefix: "runtimePanel" });
  const queryClient = useQueryClient();
  const queryKey = ["talos", scope, "worker-runtime"] as const;
  const { data, error } = useQuery({
    queryKey,
    queryFn: () => workerRuntimeClient().read(),
  });

  return (
    <Panel className="space-y-5 rounded-[var(--radius-panel)] border-border/80 bg-card/[0.82] p-5">
      <p className="text-sm leading-6 text-muted">{t("description")}</p>
      {error ? (
        <p className="text-sm text-danger" role="alert">{userVisibleErrorMessage(error, t("requestFailed"))}</p>
      ) : !data ? (
        <p className="text-sm text-muted" role="status">{t("loading")}</p>
      ) : (
        <>
          <dl className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 border-b border-divider pb-4 text-sm">
            <div className="flex items-center gap-3">
              <dt className="text-muted">{t("activeBackend")}</dt>
              <dd className="font-medium text-ink">{t(`backend.${data.activeBackend ?? "disabled"}`)}</dd>
            </div>
            {data.restartRequired ? (
              <div className="flex items-center gap-3">
                <dt className="text-muted">{t("savedBackend")}</dt>
                <dd className="font-medium text-ink">{t(`backend.${data.configuredBackend ?? "disabled"}`)}</dd>
              </div>
            ) : null}
          </dl>
          {data.restartRequired ? (
            <p className="text-sm text-muted" role="status">{t("restartRequired")}</p>
          ) : null}
          {data.configuredBackend && data.kubernetes ? (
            <RuntimeForm
              dockerSocketPath={data.dockerSocketPath ?? ""}
              key={scope}
              kubernetes={data.kubernetes}
              onSaved={(settings) => queryClient.setQueryData(queryKey, settings)}
              savedBackend={data.configuredBackend}
            />
          ) : (
            <p className="text-sm text-muted">{t("operatorSetup")}</p>
          )}
        </>
      )}
    </Panel>
  );
}

type Feedback =
  | { type: "validated" | "saved" }
  | { type: "error"; error: unknown };

function RuntimeForm({
  savedBackend,
  dockerSocketPath,
  kubernetes,
  onSaved,
}: {
  savedBackend: WorkerRuntimeBackend;
  dockerSocketPath: string;
  kubernetes: WorkerKubernetesSettings;
  onSaved: (settings: WorkerRuntimeSettingsResponse) => void;
}) {
  const { t } = useTranslation("settings", { keyPrefix: "runtimePanel" });
  const [backend, setBackend] = useState(savedBackend);
  const [namespace, setNamespace] = useState(kubernetes.namespace);
  const [storageClass, setStorageClass] = useState(kubernetes.storageClass);
  const [source, setSource] = useState(kubernetes.authenticationSource);
  const [context, setContext] = useState(kubernetes.context ?? "");
  const [busy, setBusy] = useState<"validate" | "save" | null>(null);
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const credential = useRef<HTMLTextAreaElement>(null);

  async function submit(action: "validate" | "save") {
    if (busy) return;
    const selectedContext = context || null;
    const authentication: WorkerKubeconfigAuthentication =
      source === "inCluster" ? { kind: source } :
        source === "mounted" ? { kind: source, context: selectedContext } :
          { kind: source, context: selectedContext, kubeconfig: credential.current?.value || null };
    const candidate: WorkerRuntimeCandidate = backend === "dockerEngine" ? { kind: backend } :
      { kind: backend, namespace, storageClass, authentication };
    setBusy(action);
    setFeedback(null);
    try {
      const client = workerRuntimeClient();
      if (action === "validate") {
        await client.validate(candidate);
        setFeedback({ type: "validated" });
      } else {
        const settings = await client.update(candidate);
        if (credential.current) credential.current.value = "";
        onSaved(settings);
        setFeedback({ type: "saved" });
      }
    } catch (error) {
      setFeedback({ type: "error", error });
    } finally {
      setBusy(null);
    }
  }

  return (
    <form onChange={() => setFeedback(null)} onSubmit={(event) => {
      event.preventDefault();
      void submit("save");
    }}>
      <fieldset className="space-y-4" disabled={busy !== null}>
        <label className="block space-y-2 text-sm text-muted">
          <span>{t("candidateBackend")}</span>
          <select className={selectClass} value={backend} onChange={(event) => setBackend(event.target.value as WorkerRuntimeBackend)}>
            <option value="dockerEngine">{t("backend.dockerEngine")}</option>
            <option value="kubernetes">{t("backend.kubernetes")}</option>
          </select>
        </label>
        {backend === "dockerEngine" ? (
          <label className="block space-y-2 text-sm text-muted">
            <span>{t("dockerSocket")}</span>
            <Input className="font-mono" readOnly value={dockerSocketPath} />
          </label>
        ) : (
          <>
            <div className="grid gap-4 sm:grid-cols-2">
              <label className="block space-y-2 text-sm text-muted">
                <span>{t("namespace")}</span>
                <Input value={namespace} onChange={(event) => setNamespace(event.target.value)} />
              </label>
              <label className="block space-y-2 text-sm text-muted">
                <span>{t("storageClass")}</span>
                <Input value={storageClass} onChange={(event) => setStorageClass(event.target.value)} />
              </label>
            </div>
            <label className="block space-y-2 text-sm text-muted">
              <span>{t("authentication")}</span>
              <select className={selectClass} value={source} onChange={(event) => setSource(event.target.value as WorkerKubeconfigSource)}>
                <option value="inCluster">{t("source.inCluster")}</option>
                <option value="mounted" disabled={!kubernetes.mountedKubeconfigPath}>{t("source.mounted")}</option>
                <option value="pasted">{t("source.pasted")}</option>
              </select>
            </label>
            {source === "mounted" ? (
              <label className="block space-y-2 text-sm text-muted">
                <span>{t("mountedPath")}</span>
                <Input className="font-mono" readOnly value={kubernetes.mountedKubeconfigPath ?? ""} />
              </label>
            ) : null}
            {source !== "inCluster" ? (
              <label className="block space-y-2 text-sm text-muted">
                <span>{t("context")}</span>
                <Input placeholder={t("currentContext")} value={context} onChange={(event) => setContext(event.target.value)} />
              </label>
            ) : null}
            {source === "pasted" ? (
              <div className="space-y-2 text-sm text-muted">
                <label htmlFor="runtime-kubeconfig">{t("kubeconfig")}</label>
                <Textarea aria-describedby="runtime-kubeconfig-help" autoCapitalize="off" autoComplete="off" className="font-mono" id="runtime-kubeconfig" ref={credential} rows={5} spellCheck={false} />
                <p className="text-xs leading-5" id="runtime-kubeconfig-help">
                  {t(kubernetes.authenticationSource === "pasted" ? "credentialSaved" : "credentialHelp")}
                </p>
              </div>
            ) : null}
          </>
        )}
        <div className="flex flex-wrap items-center gap-3">
          <Button type="button" variant="secondary" onClick={() => void submit("validate")}>
            {t(busy === "validate" ? "validating" : "validate")}
          </Button>
          <Button type="submit">{t(busy === "save" ? "saving" : "save")}</Button>
        </div>
      </fieldset>
      {feedback ? (
        feedback.type === "error" ? (
          <p className="mt-3 text-sm text-danger" role="alert">{userVisibleErrorMessage(feedback.error, t("requestFailed"))}</p>
        ) : (
          <p className="mt-3 text-sm text-muted" role="status">{t(feedback.type)}</p>
        )
      ) : null}
    </form>
  );
}
