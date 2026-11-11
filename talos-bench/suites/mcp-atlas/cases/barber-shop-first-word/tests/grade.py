"""Run the upstream scorer with a loopback judge for this one controlled claim."""

import csv
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import platform
import subprocess
import sys
import threading

TASK = "barber-shop-first-word"
CLAIM = "The first word is Customer."
JUDGE = "atlas-controlled-judge"


class Judge(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if (self.path != "/v1/chat/completions" or body["model"] != JUDGE
                or self.headers.get("Authorization") != "Bearer local-judge"):
            self.send_error(400, "Unexpected controlled judge request")
            return
        prompt = body["messages"][0]["content"]
        _, claim_marker, claim_section = prompt.partition("CLAIM TO EVALUATE:\n")
        claim, response_marker, response_section = claim_section.partition("\nMODEL RESPONSE TO ANALYZE:\n")
        response, instructions_marker, _ = response_section.rpartition("\nINSTRUCTIONS:\n")
        if not (claim_marker and response_marker and instructions_marker and claim == CLAIM):
            self.send_error(400, "Unexpected scorer prompt or claim")
            return
        verdict = {
            "claim_text": claim,
            "coverage_outcome": "fulfilled" if response.strip() == "Customer" else "not_fulfilled",
            "justification": "The case asks for the exact first word.",
            "confidence_level": 1,
        }
        self.server.verdicts.append(verdict)
        payload = {"choices": [{"message": {"content": json.dumps(verdict)}}]}
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(payload).encode())


def grade(answer_file, logs):
    if platform.machine() != "aarch64":
        raise RuntimeError("MCP-Atlas verification requires native ARM64")
    # Linux can expose inactive tunnel devices even with Docker network_mode none.
    active = {
        flags.parent.name for flags in Path("/sys/class/net").glob("*/flags")
        if int(flags.read_text(), 16) & 1  # IFF_UP
    }
    if active != {"lo"}:
        raise RuntimeError("The controlled verifier requires loopback-only networking")
    logs.mkdir(parents=True, exist_ok=True)
    (logs / "architecture.txt").write_text("aarch64\n")
    (logs / "network.txt").write_text("lo\n")
    response = answer_file.read_text().strip()
    model_file = logs / "candidate.csv"
    with model_file.open("w", newline="") as stream:
        csv.writer(stream).writerows([("task_id", "response"), (TASK, response)])

    with ThreadingHTTPServer(("127.0.0.1", 0), Judge) as server:
        server.verdicts = []
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            subprocess.run([
                sys.executable, "/opt/atlas/score_claims.py",
                "--groundtruth-file", str(Path(__file__).with_name("groundtruth.csv")),
                "--model-file", str(model_file), "--model-name", "talos",
                "--evaluator-model", JUDGE, "--api-key", "local-judge",
                "--base-url", f"http://127.0.0.1:{server.server_port}",
                "--output-dir", str(logs), "--concurrency", "1",
            ], check=True, timeout=30)
        finally:
            server.shutdown()
            thread.join()
        verdicts = server.verdicts

    (logs / "judge-verdicts.json").write_text(json.dumps(verdicts, indent=2) + "\n")
    with (logs / "scored_talos.csv").open(newline="") as stream:
        row, = csv.DictReader(stream)
    if row["TASK"] != TASK or row["total_claims"] != "1":
        raise ValueError("The upstream scorer did not score exactly the controlled claim")
    # The upstream scorer skips judge calls for blank and ERROR responses.
    if len(verdicts) != (0 if not response or response.startswith("ERROR:") else 1):
        raise ValueError("Unexpected number of local judge calls")
    score = float(row["coverage_score"])
    if score not in {0.0, 1.0}:
        raise ValueError("Unexpected coverage score for the controlled claim")
    (logs / "reward.txt").write_text(f"{score}\n")


if __name__ == "__main__":
    grade(*(Path(argument) for argument in sys.argv[1:]))
