"""Exercise real Web/Desktop error paths against isolated backends and a local provider.

Run with `uv run --with playwright --with pyyaml python talos-app/scripts/error-contract-e2e.py`.
Build app-server, talos-server, and Desktop main first. No external model calls.
"""
import contextlib
import json
import os
from pathlib import Path
import re
import socket
import sqlite3
import signal
import subprocess
import tempfile
import threading
import time
import yaml
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from playwright.sync_api import sync_playwright, expect

ROOT = Path(__file__).resolve().parents[2]
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
SENTINEL = "SECRET_PROVIDER_BODY_must_not_be_disclosed"


class Provider(BaseHTTPRequestHandler):
    status = 500
    success = False

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        if self.success and not request.get("stream"):
            body = json.dumps({"id": "fixture", "object": "chat.completion", "created": 1, "model": "error-fixture", "choices": [{"index": 0, "message": {"role": "assistant", "content": '{"operation":"noop"}'}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}}).encode()
        elif self.success:
            chunks = [
                {"id": "fixture", "object": "chat.completion.chunk", "created": 1, "model": "error-fixture", "choices": [{"index": 0, "delta": {"role": "assistant", "content": "Successful fixture answer."}, "finish_reason": None}]},
                {"id": "fixture", "object": "chat.completion.chunk", "created": 1, "model": "error-fixture", "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}},
            ]
            body = ("".join(f"data: {json.dumps(chunk)}\n\n" for chunk in chunks) + "data: [DONE]\n\n").encode()
        elif self.status == 200:
            body = ('data: {"error":{"code":"server_error","message":"' + SENTINEL + '"}}\n\n').encode()
        else:
            body = json.dumps({"error": {"message": SENTINEL}}).encode()
        self.send_response(self.status)
        self.send_header("Content-Type", "text/event-stream" if self.status == 200 and request.get("stream") else "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def wait_port(port, process):
    for _ in range(600):
        if process.poll() is not None:
            raise RuntimeError(f"Process exited before listening on {port}: {process.returncode}")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return
        except OSError:
            time.sleep(0.1)
    raise TimeoutError(f"Listener did not start: {port}")


def failure_reports(log, code):
    # Other concurrent requests can fail independently of this provider scenario.
    records = re.split(r"(?=^\d{4}-\d{2}-\d{2}T)", log, flags=re.MULTILINE)
    return [record for record in records
            if "talos::diagnostic: application failure " in record.split("\n", 1)[0]
            and f" code={code} " in record.split("\n", 1)[0]]


def run():
    artifacts_parent = ROOT / ".tmp"
    artifacts_parent.mkdir(exist_ok=True)
    artifacts = Path(tempfile.mkdtemp(prefix="error-contract-", dir=artifacts_parent))
    print(f"Artifacts: {artifacts}", flush=True)
    processes = []
    files = []
    provider = ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    threading.Thread(target=provider.serve_forever, daemon=True).start()

    def spawn(args, env, name, cwd=ROOT):
        log = open(artifacts / f"{name}.log", "w")
        files.append(log)
        process = subprocess.Popen(args, cwd=cwd, env=env, stdout=log, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        processes.append(process)
        return process

    def environment(name, gateway_port):
        home = artifacts / name
        home.mkdir()
        config = yaml.safe_load((ROOT / "conf/talos-dev.yaml").read_text())
        fixture = {
            "workspaces": {"default_workspace_id": "default", "definitions": [{"workspace_id": "default", "root_prefix": "workspace"}]},
            "gateway": {"host": "127.0.0.1", "port": gateway_port, "auth": False},
            "llm": {"provider": "openai", "model": "error-fixture", "openai": {
                "api_key": "dummy-fixture-key", "base_url": f"http://127.0.0.1:{provider.server_port}/v1",
                "protocol": "chat", "models": [{"id": "error-fixture", "name": "Error Fixture",
                    "context_window": 32768, "input": ["text"], "reasoning": False}],
            }},
            "agent": {"name": "talos", "inner_model": {"provider": "openai", "id": "error-fixture"}, "kernel": {
                "plugins": {"sources": [], "roots": []}, "mcp": {"servers": []},
                "output": {"max_tokens": 1024, "upper_limit": 2048},
            }},
        }
        config["gateway"].update(fixture["gateway"])
        config["llm"] = fixture["llm"]
        config["agent"]["inner_model"] = fixture["agent"]["inner_model"]
        config["agent"]["kernel"].update(fixture["agent"]["kernel"])
        (home / "talos.yaml").write_text(json.dumps(config))
        (home / ".env").write_text("")
        return {"PATH": os.environ["PATH"], "HOME": str(home), "TALOS_HOME": str(home),
                "TALOS_CONFIG": str(home / "talos.yaml"), "TALOS_RESOURCE_DIR": str(ROOT)}

    try:
        api_port, web_port, debug_port = free_port(), free_port(), free_port()
        web_env = environment("web-home", api_port)
        backend = spawn([str(ROOT / "talos-rs/target/debug/talos-server")], web_env, "web-backend", artifacts / "web-home")
        wait_port(api_port, backend)
        frontend_env = {"PATH": os.environ["PATH"], "HOME": str(artifacts / "web-home"),
                        "VITE_API_PROXY_TARGET": f"http://127.0.0.1:{api_port}"}
        frontend = spawn(["pnpm", "--filter", "talos-web", "dev", "--port", str(web_port)], frontend_env, "frontend", ROOT / "talos-app")
        wait_port(web_port, frontend)
        desktop_env = environment("desktop-home", free_port())
        desktop_env.update({"TALOS_DESKTOP_RENDERER_URL": f"http://127.0.0.1:{web_port}",
                            "TALOS_APP_SERVER_BIN": str(ROOT / "talos-rs/target/debug/app-server")})
        electron = spawn([str(ROOT / "talos-app/desktop/node_modules/.bin/electron"),
                          str(ROOT / "talos-app/desktop"), f"--remote-debugging-port={debug_port}"],
                         desktop_env, "desktop", artifacts / "desktop-home")
        wait_port(debug_port, electron)
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch(executable_path=CHROME, headless=True)
            desktop = playwright.chromium.connect_over_cdp(f"http://127.0.0.1:{debug_port}")
            page = browser.new_page(viewport={"width": 1280, "height": 900})
            page.goto(f"http://127.0.0.1:{web_port}")
            desktop_page = desktop.contexts[0].pages[0]
            for mode, current in [("web", page), ("desktop", desktop_page)]:
                current.wait_for_load_state("networkidle")
                current.screenshot(path=str(artifacts / f"{mode}-initial.png"))
                (artifacts / f"{mode}-initial.txt").write_text(current.locator("body").inner_text())
                print(f"{mode}: {current.locator('body').inner_text()[:1200]}", flush=True)
                # Discover the actual composer before exercising it.
                composer = current.locator("textarea").first
                expect(composer).to_be_visible(timeout=30000)
                log_path = artifacts / f"{mode}-home" / "logs/talos.log"
                for status in [401, 500, 200]:
                    Provider.success = False
                    Provider.status = status
                    code = "ProviderUnavailable" if status == 401 else "InternalError"
                    log_offset = log_path.stat().st_size
                    composer.fill(f"Report fixture failure {status}")
                    composer.press("Enter")
                    expected = "configured credentials" if status == 401 else (
                        "HTTP 500" if status == 500 else "remote details were withheld")
                    expect(current.get_by_text(expected, exact=False).first).to_be_visible(timeout=60000)
                    assert SENTINEL not in current.locator("body").inner_text()
                    current.screenshot(path=str(artifacts / f"{mode}-{status}.png"))
                    for _ in range(100):
                        with log_path.open("rb") as log_file:
                            log_file.seek(log_offset)
                            diagnostic = log_file.read().decode()
                        reports = failure_reports(diagnostic, code)
                        if reports and "backtrace_status=Captured" in reports[0]:
                            break
                        time.sleep(0.1)
                    assert len(reports) == 1, diagnostic
                    assert expected in reports[0] and "backtrace_status=Captured" in reports[0], reports[0]
                    replay_offset = log_path.stat().st_size
                    print(f"{mode}: visible {status} failure", flush=True)
                    current.reload()
                    # Persistent SSE is intentionally never network-idle after session recovery.
                    current.wait_for_load_state("domcontentloaded")
                    expect(current.get_by_text(expected, exact=False).first).to_be_visible(timeout=30000)
                    with log_path.open("rb") as log_file:
                        log_file.seek(replay_offset)
                        assert not failure_reports(log_file.read().decode(), code), "Replay logged the same failure again"
                    composer = current.locator("textarea").first
                # Fail one real completion effect without changing production code or Agent success.
                database = artifacts / f"{mode}-home" / "data/talos.db"
                with sqlite3.connect(database) as connection:
                    connection.execute("CREATE TRIGGER fixture_usage_failure BEFORE INSERT ON profile_turn_usage WHEN NEW.usage_kind = 'turn' BEGIN SELECT RAISE(FAIL, 'fixture usage write failed'); END")
                Provider.status, Provider.success = 200, True
                log_offset = log_path.stat().st_size
                composer.fill("Produce a successful fixture answer with a usage warning.")
                composer.press("Enter")
                warning = current.locator('[data-notice-level="warning"]').filter(has_text="insert profile turn usage")
                expect(current.get_by_text("Successful fixture answer.", exact=True)).to_be_visible(timeout=60000)
                expect(warning).to_have_count(1, timeout=30000)
                expect(warning).to_contain_text("insert profile turn usage")
                current.screenshot(path=str(artifacts / f"{mode}-success-warning.png"))
                current.reload()
                current.wait_for_load_state("domcontentloaded")
                expect(current.get_by_text("Successful fixture answer.", exact=True)).to_be_visible(timeout=30000)
                expect(current.locator('[data-notice-level="warning"]').filter(has_text="insert profile turn usage")).to_have_count(1)
                with log_path.open("rb") as log_file:
                    log_file.seek(log_offset)
                    reports = failure_reports(log_file.read().decode(), "InternalError")
                assert len(reports) == 1 and "fixture usage write failed" in reports[0] and "backtrace_status=Captured" in reports[0], reports
                with sqlite3.connect(database) as connection:
                    connection.execute("DROP TRIGGER fixture_usage_failure")
                    notices = connection.execute("SELECT payload_json FROM agent_events WHERE sequence IS NULL").fetchall()
                    assert len(notices) == 1 and json.loads(notices[0][0])["content"]["data"]["code"] == "internalError", notices
                Provider.success = False
                print(f"{mode}: success retained beside one durable typed warning, including reload", flush=True)
                if mode == "desktop":
                    result = current.evaluate("""async () => {
                      const { getTalosClient } = await import('/src/shared/api/client.ts');
                      const { TalosApiError } = await import('/@fs/""" + str(ROOT / "talos-app/ts-sdk/packages/core/src/index.ts") + """');
                      try { await getTalosClient().sessions.get({sessionId: 'missing-error-fixture'}); }
                      catch (error) { return {typed: error instanceof TalosApiError, code: error.code, message: error.message}; }
                    }""")
                    assert result == {
                        "typed": True,
                        "code": "resourceNotFound",
                        "message": "Session missing-error-fixture was not found.",
                    }, result
                    print(f"Desktop IPC preserved class/code/message: {result}", flush=True)
            browser.close()
        for name in ["web-home", "desktop-home"]:
            log = (artifacts / name / "logs/talos.log").read_text()
            assert "application failure" in log and "backtrace_status=Captured" in log
            assert SENTINEL not in log
        print("Real Web/Desktop request, provider, streamed failure, success with warning, replay, IPC and diagnostics passed.", flush=True)
    finally:
        for process in reversed(processes):
            with contextlib.suppress(ProcessLookupError):
                os.killpg(process.pid, signal.SIGTERM)
            with contextlib.suppress(subprocess.TimeoutExpired):
                process.wait(timeout=10)
        provider.shutdown()
        for file in files:
            file.close()


if __name__ == "__main__":
    run()
