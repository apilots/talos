import type { FileContentResponse } from "./generated/app-protocol";
import type { FileDownloadResponse } from "./types";

export function decodeFileContent(response: FileContentResponse): FileDownloadResponse {
  return {
    file: response.file,
    data: Uint8Array.from(atob(response.dataBase64), (character) => character.charCodeAt(0)),
  };
}
