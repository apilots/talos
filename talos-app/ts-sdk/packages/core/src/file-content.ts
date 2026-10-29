import type { FileContentResponse } from "./generated/app-protocol";
import type { FileDownloadResponse } from "./types";

export function decodeFileContent(response: FileContentResponse): FileDownloadResponse {
  return {
    file: response.file,
    data: decodeFileBytes(response.dataBase64),
  };
}

export function decodeFileBytes(base64: string): Uint8Array {
  return Uint8Array.from(atob(base64), character => character.charCodeAt(0));
}
