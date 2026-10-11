"""Isolated built Web frontend with the production SPA history fallback."""
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import sys
from urllib.parse import urlsplit

root = Path(sys.argv[2])
class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(root), **kwargs)
    def send_head(self):
        if not (root / urlsplit(self.path).path.lstrip('/')).exists():
            self.path = '/index.html'
        return super().send_head()

ThreadingHTTPServer(('127.0.0.1', int(sys.argv[1])), Handler).serve_forever()
