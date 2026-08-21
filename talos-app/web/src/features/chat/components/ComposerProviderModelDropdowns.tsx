import { useEffect, useRef, useState } from "react";
import type React from "react";
import { ChevronDown } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import type { DropdownItem } from "./ComposerDropdown";
import {
  type ComposerModelOption,
  useComposerModelOptions,
} from "./ComposerModelSelect";
import {
  ComposerProviderModelMenuRow,
  ComposerProviderModelSelectionPage,
} from "./ComposerProviderModelMenu";

type SelectionMenu = "provider" | "model" | "effort";

interface ComposerProviderModelDropdownsProps {
  disabled?: boolean;
  effortLabel: string;
  modelLabel: string;
  models: ComposerModelOption[];
  onChange?: (model: string) => void;
  onEffortChange?: (effort: string) => void;
  onOpenChange?: (open: boolean) => void;
  providerLabel: string;
  reasoningItems: DropdownItem[];
  selectedEffort: string;
  selectedModel?: string;
  modelTriggerRef?: React.Ref<HTMLButtonElement>;
}

export function ComposerProviderModelDropdowns({
  disabled = false,
  effortLabel,
  modelLabel,
  models,
  onChange,
  onEffortChange,
  onOpenChange,
  providerLabel,
  reasoningItems,
  selectedEffort,
  selectedModel,
  modelTriggerRef,
}: ComposerProviderModelDropdownsProps) {
  const { activeModel, activeProvider, changeProvider, providerModels, providers } =
    useComposerModelOptions(models, selectedModel, onChange);
  const [open, setOpen] = useState(false);
  const [selectionMenu, setSelectionMenu] = useState<SelectionMenu>();
  const ref = useRef<HTMLDivElement>(null);
  const activeModelLabel = models.find((model) => model.id === activeModel)?.name ?? modelLabel;
  const activeEffortLabel = reasoningItems.find((item) => item.value === selectedEffort)?.label
    ?? selectedEffort;

  useEffect(() => {
    onOpenChange?.(open);
  }, [onOpenChange, open]);

  useEffect(() => {
    if (!open) {
      return;
    }

    function onPointerDown(event: PointerEvent) {
      if (ref.current && !ref.current.contains(event.target as Node)) {
        setOpen(false);
        setSelectionMenu(undefined);
      }
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") {
        return;
      }
      if (!selectionMenu) {
        setOpen(false);
      } else {
        setSelectionMenu(undefined);
      }
    }

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open, selectionMenu]);

  useEffect(() => {
    if (disabled) {
      setOpen(false);
      setSelectionMenu(undefined);
    }
  }, [disabled]);

  return (
    <div className="relative" ref={ref}>
      <button
        aria-expanded={open}
        aria-label={`${activeModelLabel} · ${activeEffortLabel}`}
        className={cn(
          "inline-flex h-8 max-w-[240px] items-center gap-1.5 rounded-full px-2.5 text-[12px] font-medium text-ink/80 outline-none transition-[background-color,color,transform] duration-150",
          "hover:bg-ink/[0.025] hover:text-ink active:scale-[0.98] active:bg-ink/[0.045] focus-visible:ring-2 focus-visible:ring-brand/20",
          "disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:bg-transparent disabled:active:scale-100",
          open && "bg-ink/[0.025] text-ink",
        )}
        disabled={disabled}
        onClick={() => {
          setOpen((value) => !value);
          setSelectionMenu(undefined);
        }}
        ref={modelTriggerRef}
        type="button"
      >
        <span className="truncate">{activeModelLabel}</span>
        <span className="text-muted">· {activeEffortLabel}</span>
        <ChevronDown
          className={cn("shrink-0 text-muted transition-transform", open && "rotate-180")}
          size={12}
        />
      </button>

      {open ? (
        <div className="absolute bottom-[calc(100%+8px)] right-0 z-50 w-[268px]">
          <div className="rounded-[1.1rem] border border-border/55 bg-card p-1.5 shadow-[0_12px_32px_rgba(0,0,0,0.075)]">
            <ComposerProviderModelMenuRow
              active={selectionMenu === "provider"}
              label={providerLabel}
              onHover={() => setSelectionMenu("provider")}
              onClick={() => setSelectionMenu((value) => value === "provider" ? undefined : "provider")}
              value={activeProvider}
            />
            <ComposerProviderModelMenuRow
              active={selectionMenu === "model"}
              label={modelLabel}
              onHover={() => setSelectionMenu("model")}
              onClick={() => setSelectionMenu((value) => value === "model" ? undefined : "model")}
              value={activeModelLabel}
            />
            <ComposerProviderModelMenuRow
              active={selectionMenu === "effort"}
              disabled={reasoningItems.length === 1 && reasoningItems[0]?.value === "none"}
              label={effortLabel}
              onHover={() => setSelectionMenu("effort")}
              onClick={() => setSelectionMenu((value) => value === "effort" ? undefined : "effort")}
              value={activeEffortLabel}
            />
          </div>
          {selectionMenu ? (
            <ComposerProviderModelSelectionPage
              items={selectionMenu === "provider"
                ? providers.map((provider) => ({ label: provider, value: provider }))
                : selectionMenu === "model"
                  ? providerModels.map((model) => ({ label: model.name, value: model.id }))
                  : reasoningItems}
              onChange={(value) => {
                if (selectionMenu === "provider") {
                  changeProvider(value);
                } else if (selectionMenu === "model") {
                  onChange?.(value);
                } else {
                  onEffortChange?.(value);
                }
                setSelectionMenu(undefined);
              }}
              value={selectionMenu === "provider"
                ? activeProvider
                : selectionMenu === "model"
                  ? activeModel
                  : selectedEffort}
            />
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
