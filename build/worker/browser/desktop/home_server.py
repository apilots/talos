"""Serve only the packaged homepage assets on a private supervised endpoint."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def main():
    root = Path(__file__).resolve().parent
    content_types = {
        'home.html': 'text/html; charset=utf-8',
        'home.js': 'text/javascript; charset=utf-8',
        'talos-logo.svg': 'image/svg+xml',
    }
    assets = {f'/{name}': (mime, (root / name).read_bytes())
              for name, mime in content_types.items()}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def respond(self, body):
            asset = assets.get(self.path)
            if asset is None or self.path != self.requestline.split()[1]:
                self.send_error(404)
                return
            mime, content = asset
            self.send_response(200)
            self.send_header('Content-Type', mime)
            self.send_header('Content-Length', str(len(content)))
            self.send_header('X-Content-Type-Options', 'nosniff')
            self.end_headers()
            if body:
                self.wfile.write(content)

        def do_GET(self):
            self.respond(True)

        def do_HEAD(self):
            self.respond(False)

    with ThreadingHTTPServer(('127.0.0.1', 0), Handler) as server:
        print(server.server_port, flush=True)
        server.serve_forever()


if __name__ == '__main__':
    main()
