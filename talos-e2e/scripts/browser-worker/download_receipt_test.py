"""Exercise the Electron fixture's real receipt selector without starting Electron."""
import json
from pathlib import Path
import subprocess
import unittest


class DownloadReceiptTests(unittest.TestCase):
    def check_receipt(self, results, expected):
        module = (Path(__file__).parent / 'download-receipt.mjs').as_uri()
        script = (f'import {{findDownloadedFile}} from {json.dumps(module)};'
                  'import assert from "node:assert/strict";'
                  f'assert.equal(findDownloadedFile({json.dumps(results)}),'
                  f'{json.dumps(expected) if expected is not None else "undefined"});')
        subprocess.run(['node', '--input-type=module', '-e', script], check=True,
                       capture_output=True, text=True)

    def download(self, content, **fields):
        return dict(toolName='mcp__talos_browser__browser_download', isError=False,
                    content=content, **fields)

    def test_pending_response_before_completed_receipt(self):
        pending = self.download([dict(type='json', value=dict(text='Download report.csv'))])
        completed = self.download([dict(type='json', value=dict(files=[
            dict(originalFilename='report.csv', fileId='persisted-report')]))])
        self.check_receipt([pending, completed], 'persisted-report')

    def test_text_encoded_file_api_receipt(self):
        receipt = json.dumps(dict(files=[dict(originalFilename='report.csv', fileId='text-receipt')]))
        self.check_receipt([self.download([dict(type='text', text=receipt)])], 'text-receipt')

    def test_page_filename_or_unrelated_tool_is_not_a_receipt(self):
        page = self.download([dict(type='json', value=dict(
            originalFilename='report.csv', fileId='page-metadata'))])
        other = dict(toolName='browser_screenshot', isError=False, content=[
            dict(files=[dict(originalFilename='report.csv', fileId='wrong-tool')])])
        self.check_receipt([page, other], None)

    def test_failed_or_incomplete_download_is_not_a_receipt(self):
        failed = self.download([dict(files=[dict(originalFilename='report.csv', fileId='failed')])])
        failed['isError'] = True
        incomplete = self.download([dict(files=[dict(originalFilename='report.csv', fileId='')])])
        self.check_receipt([failed, incomplete], None)
