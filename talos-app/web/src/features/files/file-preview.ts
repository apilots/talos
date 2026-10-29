import type { FileInfo } from "@talos/sdk-browser";
import { isMarkdownFile } from "./api";

export type FileDisplay = { kind: "markdown" | "text" } | { kind: "browser"; mimeType: string } | { kind: "unsupported" };
/** Browser-native formats and existing text formatting only; storage supports more types. */
export function fileDisplay(file: Pick<FileInfo, "originalFilename" | "mimeType" | "sizeBytes">): FileDisplay {
  if (isMarkdownFile(file)) return { kind: file.sizeBytes > 10 * 1024 * 1024 ? "text" : "markdown" };
  const mime = file.mimeType.split(";")[0].trim().toLowerCase();
  if (mime === "text/html") return { kind: "browser", mimeType: "text/html" };
  if (mime.startsWith("image/") || mime.startsWith("audio/") || mime.startsWith("video/") || mime === "application/pdf") return { kind: "browser", mimeType: mime };
  if (mime.startsWith("text/") || ["application/json", "application/xml", "application/javascript"].includes(mime)) return { kind: "text" };
  return { kind: "unsupported" };
}
