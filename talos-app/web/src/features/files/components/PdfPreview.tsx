import { useEffect, useMemo, useRef, useState } from "react";
import { Document, Page, pdfjs } from "react-pdf";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { ChevronLeft, ChevronRight, Minus, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import type { FilePreviewPosition } from "@/shared/store/workspace-window";
import "react-pdf/dist/Page/AnnotationLayer.css";
import "react-pdf/dist/Page/TextLayer.css";
pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
export default function PdfPreview({ url, resourcePath, position, onPositionChange, onOpenLink }: { url: string; resourcePath: string; position?: FilePreviewPosition; onPositionChange?: (position: FilePreviewPosition) => void; onOpenLink: (url: string) => Promise<void> }) {
  const { t } = useTranslation("files");
  const saved = position?.kind === "pdf" && position.resourcePath === resourcePath ? position : undefined;
  const [page, setPage] = useState(saved?.page ?? 1);
  const [scale, setScale] = useState(saved?.scale ?? 1);
  const [linkError, setLinkError] = useState<unknown>();
  const [pages, setPages] = useState(0);
  const container = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(400);
  useEffect(() => {
    if (!container.current) return;
    const resize = new ResizeObserver(([entry]) => { if (entry.contentRect.width) setWidth(Math.max(160, entry.contentRect.width - 32)); });
    resize.observe(container.current); return () => resize.disconnect();
  }, []);
  const options = useMemo(() => {
    const base = new URL(`${import.meta.env.BASE_URL}pdf/`, window.location.href).href;
    return { cMapUrl: `${base}cmaps/`, wasmUrl: `${base}wasm/`, standardFontDataUrl: `${base}standard_fonts/` };
  }, []);
  const file = useMemo(() => ({ url, withCredentials: false }), [url]);
  const update = (page: number, scale: number) => { setPage(page); setScale(scale); onPositionChange?.({ kind: "pdf", resourcePath, page, scale }); };
  return <div className="flex min-h-0 flex-1 flex-col">
    <div className="flex shrink-0 items-center justify-center gap-3 border-b border-divider p-2 text-xs">
      <button aria-label={t("preview.previousPage")} disabled={page <= 1} onClick={() => update(page - 1, scale)} className="rounded p-1 disabled:opacity-35"><ChevronLeft size={16} /></button>
      <span>{page} / {pages || "—"}</span>
      <button aria-label={t("preview.nextPage")} disabled={!pages || page >= pages} onClick={() => update(page + 1, scale)} className="rounded p-1 disabled:opacity-35"><ChevronRight size={16} /></button>
      <button aria-label={t("preview.zoomOut")} disabled={scale <= 0.5} onClick={() => update(page, Math.max(0.5, scale - 0.25))} className="rounded p-1 disabled:opacity-35"><Minus size={15} /></button>
      <span>{Math.round(scale * 100)}%</span>
      <button aria-label={t("preview.zoomIn")} disabled={scale >= 3} onClick={() => update(page, Math.min(3, scale + 0.25))} className="rounded p-1 disabled:opacity-35"><Plus size={15} /></button>
    </div>
    {linkError ? <p role="alert" className="px-4 py-2 text-xs text-danger">{userVisibleErrorMessage(linkError, t("preview.loadFailed"))}</p> : null}
    <div ref={container} className="min-h-0 flex-1 overflow-auto bg-hover/40 p-4" onClickCapture={event => {
      const anchor = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>("a[href]") : null;
      if (!anchor || anchor.getAttribute("href")?.startsWith("#")) return;
      event.preventDefault(); setLinkError(undefined);
      void onOpenLink(anchor.href).catch(setLinkError);
    }}>
      <Document file={file} options={options} suspense={false} onItemClick={({ pageNumber }) => { if (pageNumber) update(pageNumber, scale); }} loading={<p role="status">{t("preview.loading")}</p>} error={<p role="alert">{t("preview.loadFailed")}</p>} onLoadSuccess={({ numPages }) => { setPages(numPages); if (page > numPages) setPage(numPages); }}>
        <Page pageNumber={page} scale={scale} width={width} loading={<p role="status">{t("preview.loading")}</p>} />
      </Document>
    </div>
  </div>;
}
