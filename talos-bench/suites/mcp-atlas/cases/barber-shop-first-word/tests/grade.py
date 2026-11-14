"""Run the pinned upstream MCP-Atlas claim scorer in the separate verifier."""

import csv
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
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


def run_scorer(groundtruth, candidate, logs, judge_model, *options, timeout):
    subprocess.run([
        sys.executable, "/opt/atlas/score_claims.py",
        "--groundtruth-file", str(groundtruth),
        "--model-file", str(candidate), "--model-name", "talos",
        "--evaluator-model", judge_model,
        "--output-dir", str(logs), "--concurrency", "2",
        *options,
    ], check=True, timeout=timeout)


def controlled_judge(groundtruth, candidate, logs):
    # Linux can expose inactive tunnel devices even with Docker network_mode none.
    active = {
        flags.parent.name for flags in Path("/sys/class/net").glob("*/flags")
        if int(flags.read_text(), 16) & 1  # IFF_UP
    }
    if active != {"lo"}:
        raise RuntimeError("The controlled verifier requires loopback-only networking")
    (logs / "network.txt").write_text("lo\n")
    with ThreadingHTTPServer(("127.0.0.1", 0), Judge) as server:
        server.verdicts = []
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            run_scorer(
                groundtruth, candidate, logs, JUDGE,
                "--api-key", "local-judge",
                "--base-url", f"http://127.0.0.1:{server.server_port}",
                timeout=30,
            )
        finally:
            server.shutdown()
            thread.join()
        return server.verdicts


def grade(answer_file, logs):
    architecture = platform.machine()
    if architecture not in {"aarch64", "x86_64"}:
        raise RuntimeError(f"Unsupported MCP-Atlas verifier architecture: {architecture}")
    logs.mkdir(parents=True, exist_ok=True)
    (logs / "architecture.txt").write_text(f"{architecture}\n")
    groundtruth = Path(__file__).with_name("groundtruth.csv")
    with groundtruth.open(newline="") as stream:
        case, = csv.DictReader(stream)
    task_id = case["TASK"]
    response = answer_file.read_text().strip()
    candidate = logs / "candidate.csv"
    with candidate.open("w", newline="") as stream:
        csv.writer(stream).writerows([("task_id", "response"), (task_id, response)])

    if task_id == TASK:
        verdicts = controlled_judge(groundtruth, candidate, logs)
        # The upstream scorer skips judge calls for blank and ERROR responses.
        if len(verdicts) != (0 if not response or response.startswith("ERROR:") else 1):
            raise ValueError("Unexpected number of local judge calls")
    else:
        run_scorer(groundtruth, candidate, logs, os.environ["EVAL_LLM_MODEL"], timeout=390)

    with (logs / "scored_talos.csv").open(newline="") as stream:
        row, = csv.DictReader(stream)
    if row["TASK"] != task_id:
        raise ValueError("The upstream scorer returned a different task")
    if task_id == TASK:
        if row["total_claims"] != "1":
            raise ValueError("The upstream scorer did not score exactly the controlled claim")
    else:
        details = json.loads(row["coverage_details_json"])
        verdicts = details.get("per_claim", [])
        # Atlas writes an apparent 0 for judge requests that exhausted their retries.
        if not verdicts or any(verdict["reason"].startswith("Evaluation failed:") for verdict in verdicts):
            raise RuntimeError("The upstream judge did not evaluate every claim")
    (logs / "judge-verdicts.json").write_text(json.dumps(verdicts, indent=2) + "\n")
    score = float(row["coverage_score"])
    if not 0.0 <= score <= 1.0 or (task_id == TASK and score not in {0.0, 1.0}):
        raise ValueError("Unexpected Atlas coverage score")
    (logs / "reward.txt").write_text(f"{score}\n")


if __name__ == "__main__":
    grade(*(Path(argument) for argument in sys.argv[1:]))
