// Inspect persisted File API receipts across asynchronous MCP collection attempts.
export function findDownloadedFile(results) {
  const findReceipt = (value) => {
    if (typeof value === 'string') {
      try { return findReceipt(JSON.parse(value)); } catch { return undefined; }
    }
    if (typeof value !== 'object' || value === null) return undefined;
    if (Array.isArray(value.files)) {
      const file = value.files.find(file => file?.originalFilename === 'report.csv'
        && typeof file.fileId === 'string' && file.fileId.length > 0);
      if (file) return file.fileId;
    }
    for (const child of Object.values(value)) {
      const found = findReceipt(child);
      if (found) return found;
    }
    return undefined;
  };
  for (const result of results) {
    if (!result.toolName.endsWith('browser_download') || result.isError) continue;
    const found = findReceipt(result.content);
    if (found) return found;
  }
  return undefined;
}
