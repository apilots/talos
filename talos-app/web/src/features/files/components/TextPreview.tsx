import { useEffect, useMemo, useRef, useState } from "react";
import { useInfiniteQuery } from "@tanstack/react-query";
import { EditorState } from "@codemirror/state";
import { EditorView, lineNumbers, keymap } from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import type { FilePreviewDescriptor } from "@talos/sdk-core";
import { useTranslation } from "react-i18next";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import type { FilePreviewPosition } from "@/shared/store/workspace-window";

/** CodeMirror owns text layout and line navigation. The SDK owns bounded byte
 * reads; TextDecoder owns encoding and split multibyte characters. */
export function TextPreview({ descriptor, resourcePath, position, onPositionChange }: { descriptor: FilePreviewDescriptor; resourcePath: string; position?: FilePreviewPosition; onPositionChange?: (position: FilePreviewPosition) => void }) {
  const { t } = useTranslation("files");
  const saved = position?.kind === "text" && (position.resourcePath === null || position.resourcePath === resourcePath) ? position : undefined;
  const [encoding, setEncoding] = useState(saved?.encoding ?? "utf-8");
  const [lineInput, setLineInput] = useState(String(saved?.line ?? 1));
  const [targetLine, setTargetLine] = useState<number | null>(saved?.line ?? 1);
  const element = useRef<HTMLDivElement>(null);
  const editor = useRef<EditorView>(null);
  const reported = useRef(position);
  const notify = (position: FilePreviewPosition) => { reported.current = position; onPositionChange?.(position); };
  const current = useRef({ encoding, targetLine, onPositionChange: notify }); current.current = { encoding, targetLine, onPositionChange: notify };
  useEffect(() => {
    if (position !== reported.current && position?.kind === "text" && (position.resourcePath === null || position.resourcePath === resourcePath)) {
      reported.current = position; setEncoding(position.encoding); setLineInput(String(position.line)); setTargetLine(position.line);
    }
  }, [position, resourcePath]);
  const content = useInfiniteQuery({
    queryKey: ["talos", "preview-text", descriptor.previewId, resourcePath], initialPageParam: 0,
    queryFn: ({ pageParam }) => getTalosClient().files.preview.readBytes({ previewId: descriptor.previewId, resourcePath, offset: pageParam, limit: 1024 * 1024 }),
    getNextPageParam: last => last.offset + last.data.byteLength < last.metadata.sizeBytes && last.data.byteLength ? last.offset + last.data.byteLength : undefined,
    retry: false, refetchOnWindowFocus: false,
  });
  const pages = content.data?.pages;
  const changed = pages?.some(page => page.metadata.sizeBytes !== pages[0].metadata.sizeBytes);
  const text = useMemo(() => {
    const decoder = new TextDecoder(encoding);
    return (pages?.map(page => decoder.decode(page.data, { stream: true })).join("") ?? "") + (content.hasNextPage ? "" : decoder.decode());
  }, [pages, encoding, content.hasNextPage]);
  useEffect(() => {
    if (!element.current) return;
    const view = new EditorView({ parent: element.current, state: EditorState.create({ extensions: [
      EditorState.readOnly.of(true), EditorView.editable.of(false), lineNumbers(), keymap.of(defaultKeymap),
      EditorView.theme({ "&": { height: "100%", background: "transparent", color: "inherit", fontSize: "12px" }, ".cm-scroller": { overflow: "auto", fontFamily: "var(--font-mono, monospace)" }, ".cm-gutters": { background: "transparent", borderRight: "1px solid var(--color-border)" } }),
      EditorView.domEventHandlers({ scroll: (_event, view) => {
        if (current.current.targetLine !== null || element.current?.closest("[hidden], [inert]")) return;
        const line = view.state.doc.lineAt(view.lineBlockAtHeight(view.scrollDOM.scrollTop).from).number;
        current.current.onPositionChange?.({ kind: "text", resourcePath, line, encoding: current.current.encoding });
      } }),
    ] }) });
    editor.current = view;
    return () => { editor.current = null; view.destroy(); };
  }, [resourcePath]);
  useEffect(() => {
    const view = editor.current;
    if (!view) return;
    const previous = view.state.doc.toString();
    if (text !== previous) view.dispatch({ changes: text.startsWith(previous) ? { from: previous.length, insert: text.slice(previous.length) } : { from: 0, to: previous.length, insert: text } });
    if (targetLine === null || content.isFetching || content.error || changed) return;
    if (targetLine > view.state.doc.lines && content.hasNextPage) { void content.fetchNextPage(); return; }
    const line = Math.min(targetLine, view.state.doc.lines);
    view.dispatch({ selection: { anchor: view.state.doc.line(line).from }, effects: EditorView.scrollIntoView(view.state.doc.line(line).from, { y: "start" }) });
    setTargetLine(null); setLineInput(String(line));
    current.current.onPositionChange?.({ kind: "text", resourcePath, line, encoding });
  }, [text, targetLine, content.isFetching, content.hasNextPage, content.fetchNextPage, content.error, changed, resourcePath, encoding]);
  return <div className="flex min-h-0 flex-1 flex-col">
    <form className="flex shrink-0 items-center gap-2 border-b border-divider px-3 py-2 text-xs" onSubmit={event => { event.preventDefault(); const line = Number(lineInput); if (Number.isInteger(line) && line >= 1) setTargetLine(line); }}>
      <label className="flex items-center gap-2">{t("preview.encoding")}<select aria-label={t("preview.encoding")} value={encoding} onChange={event => { setEncoding(event.target.value as typeof encoding); setTargetLine(editor.current ? editor.current.state.doc.lineAt(editor.current.lineBlockAtHeight(editor.current.scrollDOM.scrollTop).from).number : 1); }} className="rounded border border-border bg-card px-2 py-1"><option value="utf-8">UTF-8</option><option value="gb18030">GB18030</option><option value="utf-16le">UTF-16 LE</option></select></label>
      <label className="ml-auto flex items-center gap-2">{t("preview.line")}<input aria-label={t("preview.line")} inputMode="numeric" value={lineInput} onChange={event => setLineInput(event.target.value)} className="w-16 rounded border border-border bg-card px-2 py-1" /></label>
      <button type="submit" className="rounded border border-border px-2 py-1">{t("preview.go")}</button>
    </form>
    {content.error || changed ? <p role="alert" className="p-4 text-sm text-danger">{userVisibleErrorMessage(content.error, t("preview.loadFailed"))}</p> : null}
    {content.isPending ? <p role="status" className="p-4 text-sm text-muted">{t("preview.loading")}</p> : null}
    <div ref={element} className="min-h-0 flex-1 text-ink" />
    {content.hasNextPage ? <button type="button" disabled={content.isFetchingNextPage} onClick={() => void content.fetchNextPage()} className="shrink-0 border-t border-divider px-4 py-2 text-xs text-brand disabled:opacity-50">{t(content.isFetchingNextPage ? "preview.loading" : "preview.loadMore")}</button> : null}
  </div>;
}
