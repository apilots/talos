// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
vi.mock("react-pdf", () => ({
  pdfjs: { GlobalWorkerOptions: {} },
  Document: ({ children, onItemClick }: { children: React.ReactNode; onItemClick: (item: { pageNumber: number }) => void }) => <div>{children}<a href="#" onClick={event => { event.preventDefault(); onItemClick({ pageNumber: 2 }); }}>Chapter two</a></div>,
  Page: ({ pageNumber }: { pageNumber: number }) => <><span>Page {pageNumber}</span><a href="https://example.test/reference">Reference</a></>,
}));
import PdfPreview from "./PdfPreview";
afterEach(cleanup);
it("routes PDF external links through the common policy and leaves internal destinations with PDF.js", () => {
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  const open = vi.fn().mockResolvedValue(undefined), position = vi.fn();
  render(<PdfPreview url="https://preview.test/report.pdf" resourcePath="report.pdf" onOpenLink={open} onPositionChange={position} />);
  fireEvent.click(screen.getByRole("link", { name: "Reference" }));
  expect(open).toHaveBeenCalledExactlyOnceWith("https://example.test/reference");
  fireEvent.click(screen.getByRole("link", { name: "Chapter two" }));
  expect(open).toHaveBeenCalledTimes(1);
  expect(screen.getByText("Page 2")).toBeVisible();
  expect(position).toHaveBeenLastCalledWith({ kind: "pdf", resourcePath: "report.pdf", page: 2, scale: 1 });
});
