import { useState } from "react";
import type { ReactNode } from "react";
import { X } from "lucide-react";

interface AddPluginMarketplaceDialogProps {
  open: boolean;
  onClose: () => void;
}

export function AddPluginMarketplaceDialog({ open, onClose }: AddPluginMarketplaceDialogProps) {
  const [source, setSource] = useState("");
  const [gitRef, setGitRef] = useState("main");
  const [sparsePaths, setSparsePaths] = useState("");
  if (!open) {
    return null;
  }
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-ink/25 px-6 backdrop-blur-[2px]">
      <div className="w-full max-w-[760px] rounded-[1.6rem] border border-border/80 bg-card p-7 shadow-[0_30px_80px_rgba(31,28,24,0.22)]">
        <div className="flex items-start justify-between gap-6">
          <div>
            <h2 className="text-[28px] font-semibold tracking-tight text-ink">
              Add plugin marketplace
            </h2>
            <p className="mt-2 text-[18px] leading-7 text-muted">
              Add from a GitHub repo, Git URL, or local folder.{" "}
              <a className="text-brand hover:underline" href="#" onClick={(event) => event.preventDefault()}>
                Learn more
              </a>
            </p>
          </div>
          <button
            className="flex size-9 items-center justify-center rounded-[0.7rem] bg-muted/10 text-muted transition hover:bg-muted/20 hover:text-ink"
            onClick={onClose}
            type="button"
          >
            <X size={18} />
          </button>
        </div>

        <div className="mt-9 space-y-5">
          <Field label="Source">
            <input
              autoFocus
              className="h-12 w-full rounded-[0.8rem] border border-border bg-background/70 px-4 text-[17px] outline-none transition placeholder:text-muted focus:border-brand"
              onChange={(event) => setSource(event.target.value)}
              placeholder="openai/plugins or git@github.com:org/repo.git"
              value={source}
            />
          </Field>
          <Field label="Git ref">
            <input
              className="h-12 w-full rounded-[0.8rem] border border-border bg-background/70 px-4 text-[17px] outline-none transition placeholder:text-muted focus:border-brand"
              onChange={(event) => setGitRef(event.target.value)}
              placeholder="main"
              value={gitRef}
            />
          </Field>
          <Field label="Sparse paths">
            <textarea
              className="min-h-28 w-full resize-y rounded-[0.8rem] border border-border bg-background/70 px-4 py-3 text-[17px] outline-none transition placeholder:text-muted focus:border-brand"
              onChange={(event) => setSparsePaths(event.target.value)}
              placeholder="plugins/codex"
              value={sparsePaths}
            />
          </Field>
        </div>

        <div className="mt-9 flex justify-end gap-3">
          <button
            className="rounded-[0.8rem] border border-border bg-card px-6 py-3 text-[17px] text-ink transition hover:bg-background"
            onClick={onClose}
            type="button"
          >
            Cancel
          </button>
          <button
            className="rounded-[0.8rem] bg-ink px-6 py-3 text-[17px] text-background transition disabled:bg-muted/40 disabled:text-background"
            disabled={!source.trim()}
            onClick={onClose}
            type="button"
          >
            Add marketplace
          </button>
        </div>
      </div>
    </div>
  );
}

function Field({ children, label }: { children: ReactNode; label: string }) {
  return (
    <label className="block">
      <span className="mb-2 block text-[17px] text-muted">{label}</span>
      {children}
    </label>
  );
}
