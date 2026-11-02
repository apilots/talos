import { useRef, useState, useEffect, useLayoutEffect, useMemo, useCallback } from "react";
import { useTranslation } from "react-i18next";
import type { AppProtocol as API } from "@talos/sdk-core";
import { traceTurns, type TraceSelection } from "../model/selectors";
import { composeLoadedTurn } from "../model/copy";
import { useSessionTracing, type TraceTab } from "./useSessionTracing";
export type SessionTracingPanelProps = {
  sessionId: string;
  visible: boolean;
  history?: API.SessionHistoryResponse;
};
export function useTracingWorkbench({
  sessionId,
  visible,
  history,
}: SessionTracingPanelProps) {
  const { t } = useTranslation("tracing");
  const [focusedTurn, setFocusedTurn] = useState<string>(),
    [storedSelection, setSelection] = useState<TraceSelection | null>(null),
    [tab, setTab] = useState<TraceTab>("summary"),
    [consumer, setConsumer] = useState("prepared"),
    [rawField, setRawField] = useState("prompt"),
    [copyStatus, setCopyStatus] = useState<string>();
  useEffect(() => {
    if (!copyStatus) return;
    const timer = window.setTimeout(() => setCopyStatus(undefined), 3000);
    return () => window.clearTimeout(timer);
  }, [copyStatus]);
  const [range, setRange] = useState("all"),
    [mobileDetail, setMobileDetail] = useState(false);
  const [narrow, setNarrow] = useState(
    () => window.matchMedia("(max-width: 760px)").matches,
  );
  const turns = useMemo(() => traceTurns(history), [history]);
  const turnIds = useMemo(() => turns.map((item) => item.identity.turnId), [turns]);
  const turnId = turns.some((turn) => turn.identity.turnId === focusedTurn)
    ? focusedTurn
    : turns.at(-1)?.identity.turnId;
  const turn = turns.find((turn) => turn.identity.turnId === turnId);
  const selection = storedSelection?.turnId === turnId ? storedSelection : null;
  const trigger = useRef<HTMLButtonElement | null>(null),
    returnFocus = useRef<HTMLElement | null>(null),
    root = useRef<HTMLElement | null>(null),
    pendingFocus = useRef(false);
  const getFocusTarget = useCallback(() => trigger.current?.isConnected
    ? trigger.current
    : root.current?.querySelector<HTMLElement>(".tracing-turn[open] > summary")
      ?? root.current?.querySelector<HTMLElement>(".tracing-turn > summary")
      ?? null, []);
  useEffect(() => {
    if (!root.current || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => {
      // Padding changes between layouts; the border box avoids threshold oscillation.
      const width = entry.borderBoxSize[0].inlineSize;
      // Hidden tabs report zero; retain the last visible layout until they reopen.
      if (width > 0) setNarrow(width <= 760);
    });
    observer.observe(root.current, { box: "border-box" });
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (selection && pendingFocus.current) {
      pendingFocus.current = false;
      if (
        narrow &&
        (document.activeElement === trigger.current ||
          document.activeElement === document.body)
      )
        root.current
          ?.querySelector<HTMLButtonElement>("[data-tracing-close]")
          ?.focus();
    }
  }, [selection, narrow]);
  const data = useSessionTracing(
    sessionId,
    turnId,
    visible,
    selection,
    tab,
    consumer,
    rawField,
    turnIds,
  );
  const initializedTurn = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (!visible || !turnId || initializedTurn.current === turnId || narrow)
      return;
    if (selection?.turnId === turnId) {
      initializedTurn.current = turnId;
      return;
    }
    const first = data.entries.find((entry) => entry.type === "modelRequest");
    if (first?.type === "modelRequest") {
      initializedTurn.current = turnId;
      setFocusedTurn(turnId);
      setSelection({
        kind: "request",
        turnId,
        modelRequestId: first.request.modelRequestId,
        callRecordId: first.request.callRecords[0]?.callRecordId,
      });
      setTab("summary");
    }
  }, [visible, turnId, data.entries, selection, narrow]);
  useLayoutEffect(() => {
    if (returnFocus.current && !selection && !mobileDetail) {
      const target = returnFocus.current;
      returnFocus.current = null;
      // The narrow ledger/Overview must be visible before its trigger can focus.
      if (document.activeElement === document.body) target.focus();
    }
    if (!narrow) setMobileDetail(false);
    else if (
      !mobileDetail &&
      document.activeElement &&
      root.current
        ?.querySelector(".tracing-inspector")
        ?.contains(document.activeElement)
    )
      getFocusTarget()?.focus();
  }, [narrow, mobileDetail, selection, getFocusTarget]);
  const select = useCallback((next: TraceSelection, button: HTMLButtonElement) => {
    trigger.current = button;
    pendingFocus.current = true;
    setMobileDetail(narrow);
    setFocusedTurn(next.turnId);
    setSelection(next);
    setTab(
      next.kind === "request"
        ? (next.view ?? "summary")
        : next.kind === "tool"
          ? "arguments"
          : "summary",
    );
    setConsumer("prepared");
  }, [narrow]);
  const close = () => {
    const focusTarget = getFocusTarget();
    setSelection(null);
    setMobileDetail(false);
    returnFocus.current = focusTarget ?? null;
  };
  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopyStatus(t("copied"));
    } catch {
      setCopyStatus(t("copyFailed"));
    }
  };
  const copyTurn = () => {
    if (turn)
      void copy(
        composeLoadedTurn(
          turn,
          data.loadedRequests,
          data.loadedTools,
          data.loadedPayloads,
          (key) => t(key),
          data.entries,
        ),
      );
  };
  const changeRange = useCallback((value: string) => {
    setRange(value);
    if (value !== "all") setFocusedTurn(value);
    setSelection(null);
    setMobileDetail(false);
  }, []);
  const ledgerSelection: TraceSelection | null =
    selection?.kind === "request"
      ? {
          ...selection,
          callRecordId: data.callRecord?.callRecordId ?? selection.callRecordId,
          view:
            tab === "prompt"
              ? "prompt"
              : tab === "response"
                ? "response"
                : undefined,
        }
      : selection;
  return {
    root,
    selection,
    ledgerSelection,
    turns,
    turn,
    turnId,
    data,
    tab,
    setTab,
    consumer,
    setConsumer,
    rawField,
    setRawField,
    copyStatus,
    range: turns.some((item) => item.identity.turnId === range) ? range : "all",
    mobileDetail,
    narrow,
    select,
    close,
    copy,
    copyTurn,
    selectTurn: (id: string) => {
      setFocusedTurn(id);
      setSelection(null);
      setMobileDetail(false);
    },
    changeRange,
    selectCallRecord: (callRecordId: string) => {
      if (selection?.kind === "request")
        setSelection({ ...selection, callRecordId });
    },
  };
}
