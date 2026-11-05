"""Real CLI foreground acceptance using the already-running Automation model fixture.

Only public CLI subprocesses create or manage automation. No database writes,
mock runtime, second fixture server, scheduling loop or real-minute matrix.
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import uuid


class Acceptance:
    def __init__(self, args):
        self.args = args
        self.children = []
        self.handles = []
        self.evidence = []
        args.report_dir.mkdir(parents=True, exist_ok=True)
        (args.home / "conf").mkdir(parents=True, exist_ok=True)
        shutil.copyfile(args.config_template, args.home / "conf/talos.yaml")
        (args.home / ".env").write_text("")
        self.env = dict(os.environ, TALOS_HOME=str(args.home),
                        TALOS_CONFIG=str(args.home / "conf/talos.yaml"),
                        TALOS_TUI_NO_BROWSER="1")

    def launch(self, label, arguments):
        stdout = (self.args.report_dir / (label + ".stdout")).open("w")
        stderr = (self.args.report_dir / (label + ".stderr")).open("w")
        self.handles.extend([stdout, stderr])
        child = subprocess.Popen([str(self.args.talos_bin), *arguments],
                                 env=self.env, stdout=stdout, stderr=stderr)
        self.children.append(child)
        return child

    def wait_exit(self, label, child):
        code = child.wait(self.args.timeout)
        if code != 0:
            diagnostic = (self.args.report_dir / (label + ".stderr")).read_text()
            raise AssertionError("{} exited {}: {}".format(label, code, diagnostic[-4000:]))

    def run(self, label, arguments):
        child = self.launch(label, arguments)
        self.wait_exit(label, child)
        return (self.args.report_dir / (label + ".stdout")).read_text()

    def value(self, label, arguments):
        return json.loads(self.run(label, arguments))

    def gate(self, marker, child, stage="work-1"):
        deadline = time.monotonic() + self.args.timeout
        while not (self.args.fixture_dir / (marker + "." + stage)).exists():
            assert child.poll() is None, "CLI exited before reaching the model work gate"
            assert time.monotonic() < deadline, "CLI did not reach the model work gate"
            time.sleep(0.1)
        assert child.poll() is None, "CLI must remain alive at the model work gate"

    def release(self, marker, stage="work-1"):
        (self.args.fixture_dir / (marker + ".release-" + stage)).touch()

    def receipt(self, label, kind):
        result = json.loads((self.args.report_dir / (label + ".stdout")).read_text())
        reference = result["createdObject"]
        assert reference["type"] == kind
        assert reference[kind + "Id"], "owner must issue an exact nonempty creation identity"
        return reference

    def seed(self, label):
        session = str(uuid.uuid4())
        marker = case_marker("pending")
        (self.args.fixture_dir / (marker + ".release-busy")).touch()
        self.run(label, ["exec", "--session", session,
                         "AUTO_CASE={} ordinary setup message".format(marker)])
        info = self.run(label + "-info", ["session", "info", session])
        assert session in info, "ordinary CLI exec must first create the existing Session"
        return session

    def goal(self):
        session = self.seed("goal-seed")
        marker = case_marker("goal")
        child = self.launch("goal-start", ["goal", "--session", session, "start",
                            "AUTO_CASE={} controlled Goal work".format(marker)])
        self.gate(marker, child)
        reference = self.receipt("goal-start", "goal")
        self.evidence.append({"case": "goal start model gate keeps CLI alive",
                              "session": session, "createdObject": reference,
                              "pid": child.pid, "alive": True})
        self.release(marker)
        self.wait_exit("goal-start", child)
        state = self.value("goal-complete", ["goal", "--session", session])
        assert state["goal"]["id"] == reference["goalId"]
        assert state["goal"]["status"] == "complete"
        frozen = self.value("goal-complete-reread", ["goal", "--session", session])
        assert state["goal"]["createdAt"] <= state["goal"]["updatedAt"]
        assert frozen["goal"]["updatedAt"] == state["goal"]["updatedAt"]
        self.evidence.append({"case": "goal normal completion exits zero", "state": state, "reread": frozen})
        return session

    def resume(self, session):
        marker = case_marker("goal")
        child = self.launch("goal-pause-start", ["goal", "--session", session, "start",
                            "AUTO_CASE={} controlled Goal work".format(marker)])
        self.gate(marker, child)
        reference = self.receipt("goal-pause-start", "goal")
        paused = self.value("goal-pause", ["goal", "--session", session, "pause"])
        assert paused["snapshot"]["goal"]["id"] == reference["goalId"]
        assert paused["snapshot"]["goal"]["status"] == "paused"
        self.release(marker)
        self.wait_exit("goal-pause-start", child)
        # The prior actual execution has settled. Reuse its same objective and
        # exact Goal ID, but require the resumed CLI to reach a new work gate.
        (self.args.fixture_dir / (marker + ".work-1")).unlink()
        (self.args.fixture_dir / (marker + ".release-work-1")).unlink()
        child = self.launch("goal-resume", ["goal", "--session", session, "resume"])
        self.gate(marker, child)
        resumed = json.loads((self.args.report_dir / "goal-resume.stdout").read_text())
        assert resumed["snapshot"]["goal"]["id"] == reference["goalId"]
        assert resumed["snapshot"]["goal"]["status"] == "active"
        self.evidence.append({"case": "goal resume keeps same Goal alive at new gate",
                              "session": session, "goalId": reference["goalId"],
                              "pid": child.pid, "alive": True})
        self.release(marker)
        self.wait_exit("goal-resume", child)
        state = self.value("goal-resume-complete", ["goal", "--session", session])
        assert state["goal"]["id"] == reference["goalId"]
        assert state["goal"]["status"] == "complete"
        self.evidence.append({"case": "resumed Goal completes and exits zero",
                              "goalId": reference["goalId"], "ownerExit": child.returncode})

    def loop(self):
        session = self.seed("loop-seed")
        marker = case_marker("fixed")
        child = self.launch("loop-start", ["loop", "--session", session, "start", "--every", "1",
                            "AUTO_CASE={} controlled Loop work; check progress every 1 minute".format(marker)])
        self.gate(marker, child)
        reference = self.receipt("loop-start", "loop")
        self.release(marker)
        deadline = time.monotonic() + self.args.timeout
        while True:
            assert child.poll() is None, "Loop owner exited after its first work"
            state = self.value("loop-observe", ["loop", "--session", session])
            task = next(item for item in state["loops"] if item["id"] == reference["loopId"])
            assert task["status"] == "active"
            assert "expiresAt" not in task, "Loop has no uniform expiry"
            if task["nextDueAt"] is not None:
                break
            assert time.monotonic() < deadline, "Loop first work did not settle"
            time.sleep(0.2)
        assert child.poll() is None, "Loop owner must survive its first normal settlement"
        self.evidence.append({"case": "loop first settlement keeps CLI alive",
                              "session": session, "createdObject": reference,
                              "state": task, "alive": True})
        # Separate actual CLI, same saved home. No in-process notification is
        # required: the foreground owner's existing read-only observer exits.
        self.run("loop-stop", ["loop", "--session", session, "stop", reference["loopId"]])
        self.wait_exit("loop-start", child)
        self.evidence.append({"case": "separate exact-ID Stop makes original CLI exit zero",
                              "createdObject": reference, "ownerExit": child.returncode})

    def model_stop(self):
        session = self.seed("dynamic-stop-seed")
        marker = case_marker("dynamicstop")
        child = self.launch("dynamic-stop", ["loop", "--session", session, "start", "--dynamic",
                            "AUTO_CASE={} controlled Loop work; model decides the next check time".format(marker)])
        self.gate(marker, child)
        reference = self.receipt("dynamic-stop", "loop")
        self.release(marker)
        self.gate(marker, child, "declared")
        self.release(marker, "declared")
        self.wait_exit("dynamic-stop", child)
        ended = self.value("dynamic-stop-ended", ["automation", "list", "--session", session, "--ended"])
        task = next(entry["loopTask"] for entry in ended["data"]
                    if entry["type"] == "loop" and entry["loopTask"]["id"] == reference["loopId"])
        assert task["stopReason"] == "modelStopped" and task["nextDueAt"] is None
        self.evidence.append({"case": "model Stop settles and releases original CLI owner",
                              "createdObject": reference, "ownerExit": child.returncode, "state": task})

    def close(self):
        for child in self.children:
            if child.poll() is None:
                child.terminate()
        for child in self.children:
            if child.poll() is None:
                try:
                    child.wait(10)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
        for handle in self.handles:
            handle.close()


def case_marker(kind):
    return kind + "_" + uuid.uuid4().hex


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--talos-bin", required=True, type=Path)
    parser.add_argument("--fixture-dir", required=True, type=Path)
    parser.add_argument("--home", required=True, type=Path)
    parser.add_argument("--config-template", required=True, type=Path)
    parser.add_argument("--report-dir", required=True, type=Path)
    parser.add_argument("--timeout", type=float, default=45)
    args = parser.parse_args()
    for name in ("talos_bin", "fixture_dir", "home", "config_template", "report_dir"):
        setattr(args, name, getattr(args, name).resolve())
    assert args.talos_bin.is_file(), "build the actual talos binary first"
    assert (args.fixture_dir / "port").is_file(), "start the original Automation model fixture first"
    assert os.environ.get("TALOS_RESOURCE_DIR"), "inherit the original scenario resource root"
    assert args.home != Path.home() / ".talos", "use an isolated acceptance home"
    assert not args.home.exists(), "CLI acceptance home must be fresh"
    assert args.timeout > 0
    acceptance = Acceptance(args)
    result = "failed"
    try:
        session = acceptance.goal()
        acceptance.resume(session)
        acceptance.loop()
        acceptance.model_stop()
        result = "passed"
        print("CASE automation_cli_foreground PASS goal=start,complete,pause,resume loop=first,exact-stop,model-stop")
    finally:
        acceptance.close()
        (args.report_dir / "result.json").write_text(json.dumps(
            {"result": result, "evidence": acceptance.evidence}, indent=2) + "\n")


if __name__ == "__main__":
    main()
