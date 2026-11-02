export function downloadFile(data: Uint8Array, filename: string, mimeType: string) {
  const url = URL.createObjectURL(new Blob([new Uint8Array(data).buffer], { type: mimeType }));
  const link = document.createElement("a");
  link.href = url; link.download = filename; document.body.append(link); link.click(); link.remove();
  // Keep the Blob alive while the browser consumes the user-initiated download.
  window.setTimeout(() => URL.revokeObjectURL(url), 30_000);
}
