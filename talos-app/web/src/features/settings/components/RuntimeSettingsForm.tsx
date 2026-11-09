import type {
  WorkerKubeconfigAuthentication,
  WorkerKubeconfigSource,
  WorkerKubernetesSettings,
  WorkerRuntimeBackend,
  WorkerRuntimeCandidate,
  WorkerRuntimeConnection,
  WorkerRuntimeProfile,
  WorkerRuntimeSettingsResponse,
} from "@talos/sdk-browser";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";
import { Input, Textarea } from "@/shared/ui/Input";
import { workerRuntimeClient } from "../api";
import { RuntimeDeploymentSettings } from "./RuntimeDeploymentSettings";

const selectClass =
  "form-control-focus w-full rounded-[var(--radius-control)] border border-border bg-card px-4 py-3 text-sm text-ink";

type Feedback =
  | { type: "validated" | "saved" }
  | { type: "error"; error: unknown };

export function RuntimeSettingsForm({
  backend,
  dockerSocketPath,
  kubernetes,
  profile,
  timing,
  onSaved,
}: {
  backend: WorkerRuntimeBackend;
  dockerSocketPath: string;
  kubernetes: WorkerKubernetesSettings;
  profile: WorkerRuntimeProfile;
  timing: NonNullable<WorkerRuntimeSettingsResponse["timing"]>;
  onSaved: (settings: WorkerRuntimeSettingsResponse) => void;
}) {
  const { t } = useTranslation("settings", { keyPrefix: "runtimePanel" });
  const [draft, setDraft] = useState<WorkerRuntimeConnection>();
  const worker = draft ?? profile.worker;
  const editWorker = (patch: Partial<WorkerRuntimeConnection>) => setDraft({ ...worker, ...patch });
  const [idleTimeout, setIdleTimeout] = useState<string>();
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
    const edits = {
      worker: draft ? { ...worker, trustedCaFile: worker.trustedCaFile?.trim() || null } : null,
      idleTimeoutSeconds: idleTimeout === undefined ? null : Number(idleTimeout),
    };
    const candidate: WorkerRuntimeCandidate = backend === "dockerEngine" ? { kind: backend, edits } :
      { kind: backend, namespace, storageClass, authentication, edits };
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
        setDraft(undefined);
        setIdleTimeout(undefined);
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
          <span>{t("workerImage")}</span>
          <Input className="font-mono" value={worker.image} onChange={(event) => editWorker({ image: event.target.value })} />
        </label>
        <label className="block space-y-2 text-sm text-muted">
          <span>{t("controlPlane")}</span>
          <Input value={worker.controlPlaneEndpoint} onChange={(event) => editWorker({ controlPlaneEndpoint: event.target.value })} />
        </label>
        <div className="space-y-2 text-sm text-muted">
          <label htmlFor="runtime-worker-ca">{t("trustedCa")}</label>
          <Input aria-describedby="runtime-worker-ca-help" className="font-mono" id="runtime-worker-ca" placeholder={t("systemTrust")} value={worker.trustedCaFile ?? ""} onChange={(event) => editWorker({ trustedCaFile: event.target.value })} />
          <p className="text-xs" id="runtime-worker-ca-help">{t("trustedCaHelp")}</p>
        </div>
        {backend === "kubernetes" ? (
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
        ) : null}
        <RuntimeDeploymentSettings
          backend={backend} dockerSocketPath={dockerSocketPath} kubernetes={kubernetes}
          profile={profile} timing={timing} idleTimeout={idleTimeout} onIdleTimeoutChange={setIdleTimeout}
        />
        <div className="flex flex-wrap items-center gap-3 border-t border-divider pt-4">
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
