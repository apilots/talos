"""A local deterministic Chat Completions endpoint for benchmark fixtures."""

from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import threading

USAGE = {"prompt_tokens": 100, "completion_tokens": 10, "total_tokens": 110}


def tool_responses(request):
    return [message for message in request["messages"] if message["role"] == "tool"]


@contextmanager
def model_config(model, respond):
    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            if self.path != "/v1/chat/completions" or body.get("model") != model:
                self.send_error(400, "Unexpected fixture endpoint or model")
                return
            if not body.get("stream"):
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({
                    "id": "fixture-title", "model": model, "created": 1,
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": "Benchmark"},
                                 "finish_reason": "stop"}], "usage": USAGE,
                }).encode())
                return
            try:
                response = respond(body)
            except AssertionError as error:
                self.send_error(400, str(error))
                return
            delta = {"role": "assistant"}
            if isinstance(response, str):
                delta["content"] = response
                finish = "stop"
            else:
                name, arguments = response
                delta["tool_calls"] = [{
                    "index": 0, "id": f"fixture_call_{len(tool_responses(body))}", "type": "function",
                    "function": {"name": name, "arguments": json.dumps(arguments)},
                }]
                finish = "tool_calls"
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            for change, reason in [(delta, None), ({}, finish)]:
                chunk = {"id": "fixture", "object": "chat.completion.chunk", "created": 1,
                         "model": model, "choices": [{"index": 0, "delta": change, "finish_reason": reason}]}
                if reason:
                    chunk["usage"] = USAGE
                self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
            self.wfile.write(b"data: [DONE]\n\n")

    with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield {
                "provider": "fixture", "model": model,
                "custom_providers": {"fixture": {
                    "name": "Benchmark fixture", "protocol": "chat", "api_key": "fixture",
                    "base_url": f"http://127.0.0.1:{server.server_port}/v1",
                    "models": [{"id": model, "name": model, "context_window": 32768}],
                }},
            }
        finally:
            server.shutdown()
            thread.join()
