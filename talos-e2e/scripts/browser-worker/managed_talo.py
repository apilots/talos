"""Secretless Talo acceptance against a disposable Server, registry and Worker."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import urllib.request
import uuid

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from fixture import ACCOUNTS, REPORT, ROOT, TOKEN, configure, data, free_port, http, wait
from lifecycle import container_args
from media import media

SERVER_IMAGE = os.environ.get("TALOS_E2E_SERVER_IMAGE", "talos-server:browser-e2e")
WORKER_IMAGE = os.environ.get("TALOS_E2E_WORKER_IMAGE", "talos-worker:browser-e2e")
RECOVERY = os.environ.get("TALOS_E2E_TALO_RECOVERY") == "1"
SOCKET = os.environ.get("TALOS_E2E_DOCKER_SOCKET", "/var/run/docker.sock")
REGISTRY_IMAGE = "registry:2@sha256:a3d8aaa63ed8681a604f1dea0aa03f100d5895b6a58ace528858a7b332415373"
ASSISTANT = "/v1/computers/managed-worker"
LABEL = "io.talos.managed-worker.instance"
FIXTURE_PROGRAM = r"""
from http.server import ThreadingHTTPServer
from threading import Event, Thread
import os, select, socket, socketserver, subprocess
import json
from fixture import Fixture, auxiliary_response

blocked = Event()

class RegistrationRelay(socketserver.BaseRequestHandler):
    def handle(self):
        if blocked.is_set():
            return
        with socket.create_connection(("127.0.0.1", 9443)) as upstream:
            streams = {self.request: upstream, upstream: self.request}
            while not blocked.is_set():
                for source in select.select(list(streams), [], [], .2)[0]:
                    payload = source.recv(65536)
                    if not payload:
                        return
                    streams[source].sendall(payload)

class ManagedFixture(Fixture):
    def do_GET(self):
        if os.environ.get("TALOS_E2E_TALO_RECOVERY") == "1" and self.path in ("/fault/drop", "/fault/restore"):
            (blocked.set if self.path.endswith("/drop") else blocked.clear)()
            return self.reply({"blocked": blocked.is_set()})
        return super().do_GET()

    def do_POST(self):
        if self.path != "/v1/chat/completions":
            return super().do_POST()
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if not request.get("stream"):
            try:
                answer = auxiliary_response(request)
            except (ValueError, KeyError, TypeError):
                answer = "TALO_WAKE_OK"
            return self.reply({
                "id": "talo-fixture", "object": "chat.completion", "created": 1,
                "model": "browser-fixture", "choices": [{"index": 0, "finish_reason": "stop",
                    "message": {"role": "assistant", "content": answer}}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
            })
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Connection", "close")
        self.end_headers()
        for delta, finish in [({"role": "assistant", "content": "TALO_WAKE_OK"}, None), ({}, "stop")]:
            chunk = {"id": "talo-fixture", "object": "chat.completion.chunk", "created": 1,
                     "model": "browser-fixture", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
            self.wfile.write(("data: " + json.dumps(chunk) + "\n\n").encode())
        self.wfile.write(b"data: [DONE]\n\n")
        self.close_connection = True

server = ThreadingHTTPServer(("0.0.0.0", 18080), ManagedFixture)
Thread(target=server.serve_forever, daemon=True).start()
if os.environ.get("TALOS_E2E_TALO_RECOVERY") == "1":
    relay = socketserver.ThreadingTCPServer(("0.0.0.0", 9444), RegistrationRelay)
    Thread(target=relay.serve_forever, daemon=True).start()
raise SystemExit(subprocess.call(["/opt/talos/bin/talos-server"]))
"""


def docker(*args, check=True):
    result = subprocess.run(["docker", *args], text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT if args[0] == "logs" else subprocess.PIPE, timeout=300)
    if check and result.returncode:
        raise RuntimeError(f"docker {args[0]} failed: {result.stderr.strip()}")
    return result.stdout.strip()


def server_files(files, gateway, rpc_port, model_port, worker_image):
    cert, key = files / "server.crt", files / "server.key"
    subprocess.run([
        "openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
        "-subj", f"/CN={gateway}", "-addext", f"subjectAltName=IP:{gateway},IP:127.0.0.1",
        "-addext", "basicConstraints=critical,CA:FALSE",
        "-addext", "keyUsage=critical,digitalSignature,keyEncipherment",
        "-addext", "extendedKeyUsage=serverAuth", "-keyout", str(key), "-out", str(cert)
    ], check=True, capture_output=True)
    key.chmod(0o644)  # Disposable fixture key, mounted read-only for the image's UID.
    profile = configure(yaml.safe_load((ROOT / "conf/talos-dev.yaml").read_text()), files, 8080, 18080)
    profile["gateway"]["host"] = "0.0.0.0"
    profile["agent_node"] = {"control_plane": {
        "bind_address": "0.0.0.0:9443",
        "tls_certificate_file": "/fixture/server.crt",
        "tls_private_key_file": "/fixture/server.key",
        "trusted_node_ca_file": "/fixture/server.crt",
    }}
    profile["cloud"].update(agent_base_url="https://127.0.0.1:9443", agent_trusted_ca_file="/fixture/server.crt")
    profile["talos"] = {
        "worker": {"runtime": {
            "kind": "DockerEngine",
            "image": worker_image,
            "control_plane_endpoint": f"https://{gateway}:{rpc_port}",
            "trusted_ca_file": "/fixture/server.crt",
        }},
        "server": {"runtime": {"idle_timeout_seconds": 12}},
    }
    (files / "talos.yaml").write_text(yaml.safe_dump(profile, sort_keys=False))
    profile["llm"]["openai"]["base_url"] = f"http://{gateway}:{model_port}/v1"
    return profile["llm"]


def managed_resource(kind, instance):
    args = ("ps", "-a") if kind == "container" else (kind, "ls")
    return docker(*args, "--filter", f"label={LABEL}={instance}", "--format", "{{.ID}}" if kind == "container" else "{{.Name}}").splitlines()


def phase(base, expected):
    snapshot = data(http(base + ASSISTANT))["instance"]
    if snapshot and snapshot["phase"] == "failed":
        raise AssertionError(f"Talo failed during {expected}: {snapshot['error']}")
    return snapshot if snapshot and snapshot["phase"] == expected else None


def exercise(base, instances, worker_model, faults):
    for token, user in ACCOUNTS.items():
        assert data(http(base + "/v1/user", token=token))["user"]["user_id"] == user["user_id"]
    bob = next(token for token in ACCOUNTS if token != TOKEN)
    assert data(http(base + ASSISTANT)) == {"enabled": True, "instance": None}
    assert http(base + ASSISTANT, {}, token="no-such-fixture-user")["status"] != "success"
    created = data(http(base + ASSISTANT, {}))["instance"]
    instance = created["instanceId"]
    instances.append(instance)
    assert data(http(base + ASSISTANT, {}))["instance"]["instanceId"] == instance
    node = created["nodeId"]
    assert data(http(base + ASSISTANT, token=bob))["instance"] is None

    def action(kind, observed, token=TOKEN, **fields):
        return http(base + f"{ASSISTANT}/{instance}/{kind}", {
            "expectedGeneration": observed["generation"],
            **fields,
        }, token=token)

    ready = wait(lambda: phase(base, "ready"), "authenticated TLS registration", seconds=130)
    assert ready["nodeId"] == node and ready["diskQuota"] == "unsupported", ready
    assert data(http(base + f"/v1/computers/{node}"))["available"]
    assert http(base + f"/v1/computers/{node}", token=bob)["status"] != "success"
    assert action("stop", ready, token=bob)["status"] != "success"
    assert action("delete", ready, confirmDataLoss=False)["status"] != "success"
    print("PASS create -> outbound TLS registration -> Ready; instance is account-scoped", flush=True)

    [worker] = managed_resource("container", instance)
    [volume] = managed_resource("volume", instance)
    first_id = docker("inspect", "--format", "{{.Id}}", worker)
    docker("exec", worker, "/bin/bash", "-ec", 'umask 077; printf t > "$TALOS_HOME/e2e-retain"')
    data(action("stop", ready))
    stopped = wait(lambda: phase(base, "stopped"), "manual stop")
    assert docker("inspect", "--format", "{{.State.Status}}", first_id) == "exited"
    assert action("start", ready)["status"] != "success"  # stale generation
    data(action("start", stopped))
    ready = wait(lambda: phase(base, "ready"), "same Worker start and registration")
    assert managed_resource("container", instance) == [worker]
    data(action("reset", ready))
    ready = wait(lambda: phase(base, "ready"), "replacement Worker registration")
    [replacement] = managed_resource("container", instance)
    assert docker("inspect", "--format", "{{.Id}}", replacement) != first_id
    assert ready["nodeId"] == node and ready["instanceId"] == instance
    assert managed_resource("volume", instance) == [volume]
    assert docker("exec", replacement, "/bin/bash", "-ec", 'cat "$TALOS_HOME/e2e-retain"') == "t"
    print("PASS manual stop, stale generation rejection, same-Worker Start and Home-preserving Reset", flush=True)

    sleeping = wait(lambda: phase(base, "sleeping"), "idle sleep", seconds=45)
    assert docker("inspect", "--format", "{{.State.Status}}", replacement) == "exited"
    directory = base + f"/v1/fs/directory?nodeId={node}"
    assert http(directory)["status"] != "success"
    assert phase(base, "sleeping")["generation"] == sleeping["generation"]
    assert data(http(directory + "&wakeIfSleeping=true"))["directory"]["readable"]
    ready = wait(lambda: phase(base, "ready"), "Worker registration after explicit directory access")
    assert ready["generation"] > sleeping["generation"] and ready["nodeId"] == node
    assert managed_resource("container", instance) == [replacement]
    print("PASS idle sleep; passive read leaves it stopped and explicit access wakes the same Worker", flush=True)

    headers = {"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"}
    request = urllib.request.Request(base + f"/v1/llm?nodeId={node}",
                                     data=json.dumps(worker_model).encode(), headers=headers, method="PUT")
    with urllib.request.urlopen(request, timeout=30) as response:
        data(json.load(response))
    wait(lambda: phase(base, "sleeping"), "idle sleep before first Turn", seconds=45)
    session = f"sess_talo_{uuid.uuid4().hex}"
    start_payload = {
        "sessionId": session,
        "nodeId": node,
        "modelSelection": {"modelRef": {"provider": "openai", "id": "browser-fixture"}, "reasoningEffort": None},
        "input": {"content": [{"type": "text", "text": "Talo first Turn after sleep"}]},
    }

    def start_first_turn():
        request = urllib.request.Request(base + "/v1/turns/start", data=json.dumps(start_payload).encode(),
            headers={**headers, "Idempotency-Key": "managed-talo-first-turn"})
        with urllib.request.urlopen(request, timeout=65) as response:
            return data(json.load(response))

    turn = start_first_turn()
    events = urllib.request.Request(base + f"/v1/sessions/{session}/events?limit=1000&turn_id={turn['turnId']}",
                                    headers=headers)
    answer = False
    with urllib.request.urlopen(events, timeout=45) as response:
        for line in response:
            if not line.startswith(b"data: "):
                continue
            record = json.loads(line[6:]).get("event", {})
            if record.get("type") != "agent":
                continue
            event = record["data"]["payload"]
            answer = answer or b"TALO_WAKE_OK" in line
            if event["type"] in {"turnCompleted", "turnFailed", "turnCancelled"}:
                assert event["type"] == "turnCompleted" and answer, event
                break
        else:
            raise AssertionError("Talo Turn did not publish a terminal event")
    ready = wait(lambda: phase(base, "ready"), "ready after first Turn")
    print("PASS first Turn wakes Talo and executes through its Worker-local model configuration", flush=True)

    media(base, node, replacement, http, data, wait, REPORT)
    if RECOVERY:
        ready = recovery(base, faults, instance, node, ready, action, worker_model, session, start_first_turn, turn)

    data(action("delete", ready, confirmDataLoss=True))
    wait(lambda: data(http(base + ASSISTANT))["instance"] is None, "Talo deletion")
    assert not any(managed_resource(kind, instance) for kind in ("container", "volume", "network"))
    assert http(base + f"/v1/computers/{node}")["status"] != "success"
    print("PASS confirmed Delete removed compute, network, Home and Computer registration", flush=True)


def recovery(base, faults, instance, node, ready, action, worker_model, session, start_first_turn, first_turn):
    evidence = []
    [worker] = managed_resource("container", instance)
    [volume] = managed_resource("volume", instance)
    check_node = lambda: data(http(base + f"/v1/computers/{node}"))
    if probe := os.environ.get("TALOS_E2E_TALO_DESKTOP_PROBE"):
        subprocess.run([probe, base, node, str(REPORT), "before-reset"], check=True, timeout=100)
        ready = wait(lambda: phase(base, "ready"), "ready after native desktop")
    http(faults + "/fault/drop")
    offline = wait(lambda: phase(base, "offline"), "Worker disconnect")
    assert offline["generation"] == ready["generation"] and not check_node()["available"]
    assert managed_resource("container", instance) == [worker]
    http(faults + "/fault/restore")
    ready = wait(lambda: phase(base, "ready"), "Worker reconnect", seconds=45)
    assert ready["generation"] == offline["generation"] and ready["nodeId"] == node
    evidence.append({"case": "worker-reconnect", "generation": ready["generation"], "sameContainer": True})
    print("PASS Worker drops offline and reconnects without replacing its container or generation", flush=True)

    sleeping = wait(lambda: phase(base, "sleeping"), "idle sleep before queued Turn", seconds=65)
    assert start_first_turn() == first_turn
    invalid_start = http(base + "/v1/turns/start", {
        "sessionId": session, "nodeId": node, "projectId": "wrong-project",
        "input": {"content": [{"type": "text", "text": "Invalid binding must not wake Talo"}]},
    })
    assert invalid_start.get("error", {}).get("code") == "invalidRequest", invalid_start
    retry = http(base + "/v1/turns/retry", {
        "sessionId": session, "clientMessageId": "invalid-retry",
        "retryOfMessageId": "missing-message",
        "input": {"content": [{"type": "text", "text": "Invalid retry must not wake Talo"}]},
    })
    assert retry.get("error", {}).get("code") == "retrySourceMismatch", retry
    assert phase(base, "sleeping")["generation"] == sleeping["generation"]
    evidence.append({"case": "turn-command-validation", "replayedOriginalTurn": True, "invalidCommandsStayedAsleep": True})
    print("PASS accepted Turn replays and invalid commands are rejected without waking Talo", flush=True)
    http(faults + "/fault/drop")
    history = base + f"/v1/sessions/{session}/history"
    queue = data(http(base + "/v1/message-queue/add", {
        "sessionId": session,
        "input": {"content": [{"type": "text", "text": "Talo queued Turn after delayed registration"}]},
    }))
    assert len(queue["messages"]) == 1
    wait(lambda: phase(base, "waitingForRegistration"), "queued Turn starts Wake")
    # Outlast an interactive request without reaching the lifecycle deadline.
    time.sleep(46)
    assert len(data(http(history))["data"]) == 1
    http(faults + "/fault/restore")

    def queued_turn_completed():
        turns = data(http(history))["data"]
        return turns if len(turns) == 2 and all(turn["result"]["status"] == "completed" for turn in turns) else None

    wait(queued_turn_completed, "queued Turn dispatches after delayed registration", seconds=45)
    assert not data(http(base + "/v1/message-queue/list", {"sessionId": session}))["messages"]
    evidence.append({"case": "queued-wake", "registrationHeldSeconds": 46, "completedTurns": 1})
    print("PASS queued User Turn survives the interactive timeout and dispatches once after registration", flush=True)

    sleeping = wait(lambda: phase(base, "sleeping"), "idle sleep before Wake timeout", seconds=65)
    http(faults + "/fault/drop")
    missed_session = f"sess_talo_timeout_{uuid.uuid4().hex}"
    request = urllib.request.Request(base + "/v1/turns/start", data=json.dumps({
        "sessionId": missed_session, "nodeId": node,
        "input": {"content": [{"type": "text", "text": "This must not be dispatched"}]},
    }).encode(), headers={"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"})
    started = time.monotonic()
    try:
        urllib.request.urlopen(request, timeout=65).close()
        raise AssertionError("Wake admitted a Turn without a registered Worker")
    except urllib.error.HTTPError as error:
        assert error.code == 504 and json.load(error)["error"]["code"] == "timeout", error
    elapsed = round(time.monotonic() - started, 2)
    assert 44 <= elapsed < 65
    assert http(base + f"/v1/sessions/{missed_session}")["status"] != "success"
    assert docker("inspect", "--format", "{{.State.Running}}", worker) == "true"

    def operation_timeout():
        snapshot = data(http(base + ASSISTANT))["instance"]
        return snapshot if snapshot["phase"] == "failed" else None

    failed = wait(operation_timeout, "overall Wake timeout", seconds=92)
    assert failed["error"]["code"] == "timeout" and failed["generation"] == sleeping["generation"] + 1
    assert docker("inspect", "--format", "{{.State.Running}}", worker) == "true"
    evidence.append({"case": "wake-timeout", "requestSeconds": elapsed,
                     "operationSeconds": round(time.monotonic() - started, 2),
                     "containerRunning": True, "turnCreated": False})
    print("PASS Wake request and operation time out with no Turn and leave the started container running", flush=True)
    http(faults + "/fault/restore")
    def late_registration():
        snapshot = data(http(base + ASSISTANT))["instance"]
        return snapshot if snapshot["phase"] == "ready" else None

    ready = wait(late_registration, "late registration", seconds=45)
    assert ready["generation"] == failed["generation"] and http(base + f"/v1/sessions/{missed_session}")["status"] != "success"
    evidence.append({"case": "late-registration", "generation": ready["generation"], "turnReplayed": False})

    http(faults + "/fault/drop")
    offline = wait(lambda: phase(base, "offline"), "offline before Reset")
    data(action("reset", offline))
    wait(lambda: phase(base, "waitingForRegistration"), "replacement starts while registration is cut")
    [replacement] = managed_resource("container", instance)
    assert replacement != worker and managed_resource("volume", instance) == [volume]
    http(faults + "/fault/restore")
    ready = wait(lambda: phase(base, "ready"), "offline Reset registration", seconds=45)
    assert ready["nodeId"] == node and ready["instanceId"] == instance
    assert docker("exec", replacement, "/bin/bash", "-ec", 'cat "$TALOS_HOME/e2e-retain"') == "t"
    llm = data(http(base + f"/v1/llm?nodeId={node}"))["config"]
    assert llm["openai"]["base_url"] == worker_model["openai"]["base_url"]
    assert data(http(base + f"/v1/sessions/{session}"))["session"]["nodeId"] == node
    evidence.append({"case": "offline-reset", "newContainer": True, "sameVolume": True,
                     "sameNode": True, "fileRetained": True, "providerRetained": True, "sessionRetained": True})
    if probe := os.environ.get("TALOS_E2E_TALO_DESKTOP_PROBE"):
        subprocess.run([probe, base, node, str(REPORT), "after-reset"], check=True, timeout=100)
        ready = wait(lambda: phase(base, "ready"), "ready after replacement desktop")
    (REPORT / "managed-talo-recovery.json").write_text(json.dumps(evidence, indent=2))
    print("PASS offline Reset replaces compute and preserves Home, Computer, model configuration and Session", flush=True)
    return ready


def server_healthy(container, base):
    if docker("inspect", "--format", "{{.State.Status}}", container) == "exited":
        raise AssertionError("Talo Server exited before accepting requests")
    return http(base + "/health")


def run(files):
    files.chmod(0o755)
    run_id = uuid.uuid4().hex[:10]
    registry, server = f"talo-e2e-registry-{run_id}", f"talo-e2e-server-{run_id}"
    instances = []
    tag = None
    try:
        docker("run", "-d", *container_args(registry), "--publish", "127.0.0.1::5000", REGISTRY_IMAGE)
        registry_address = docker("port", registry, "5000/tcp")
        # The Engine trusts loopback registries. The API requires an actual immutable manifest digest.
        tag = f"{registry_address}/talo-e2e-worker:fixture"
        docker("tag", WORKER_IMAGE, tag)
        docker("push", tag)
        digests = json.loads(docker("image", "inspect", tag, "--format", "{{json .RepoDigests}}"))
        pinned_worker = next(item for item in digests if item.startswith(tag.rsplit(":", 1)[0] + "@sha256:"))
        gateway = docker("network", "inspect", "bridge", "--format", "{{(index .IPAM.Config 0).Gateway}}")
        rpc_port, model_port = free_port(), free_port()
        fault_port = free_port()
        recovery_args = ["--publish", f"127.0.0.1:{fault_port}:18080",
                         "--env", "TALOS_E2E_TALO_RECOVERY=1"] if RECOVERY else []
        ingress_port = 9444 if RECOVERY else 9443
        worker_model = server_files(files, gateway, rpc_port, model_port, pinned_worker)
        socket_mount = f"type=bind,src={SOCKET},dst=/var/run/docker.sock"
        gid = docker("run", "--rm", "--network", "none", "--mount", socket_mount, SERVER_IMAGE,
                     "stat", "-Lc", "%g", "/var/run/docker.sock")
        mounts = [
            "--mount", socket_mount,
            "--mount", f"type=bind,src={ROOT / 'talos-e2e'},dst=/workspace/talos-e2e,readonly",
            "--mount", f"type=bind,src={files},dst=/fixture,readonly",
        ]
        if web := os.environ.get("TALOS_WEBUI_DIR"):
            mounts += ["--mount", f"type=bind,src={Path(web).resolve()},dst=/fixture-web,readonly",
                       "--env", "TALOS_WEBUI_DIR=/fixture-web"]
        if binary := os.environ.get("TALOS_E2E_TALO_SERVER_BIN"):
            mounts += [
                "--mount", f"type=bind,src={Path(binary).resolve()},dst=/opt/talos/bin/talos-server,readonly",
                "--mount", f"type=bind,src={ROOT / 'conf/migrations'},dst=/opt/talos/conf/migrations,readonly",
            ]
        docker("run", "-d", *container_args(server), "--group-add", gid, *mounts,
               "--publish", "127.0.0.1::8080", "--publish", f"{gateway}:{rpc_port}:{ingress_port}",
               "--publish", f"{gateway}:{model_port}:18080", *recovery_args,
               "--security-opt", "no-new-privileges:true", "--security-opt",
               "seccomp=" + str(ROOT / "deploy/docker/talos-worker/chromium-seccomp.json"),
               "--shm-size", "1g", "--env", "TALOS_CONFIG=/fixture/talos.yaml",
               "--env", "TALOS_HOME=/home/talos/.talos",
               "--env", "TALOS_E2E_TMP_ROOT=/tmp/talo-e2e",
               "--env", "TALOS_SCENARIO_REPORT_DIR=/tmp/talo-e2e",
               "--env", "PYTHONPATH=/workspace/talos-e2e/scripts:/workspace/talos-e2e/scripts/browser-worker",
               SERVER_IMAGE, "python3", "-c", FIXTURE_PROGRAM)
        base = "http://" + docker("port", server, "8080/tcp")
        wait(lambda: server_healthy(server, base), "Server health")
        exercise(base, instances, worker_model, f"http://127.0.0.1:{fault_port}")
    finally:
        (REPORT / "managed-talo-server.log").write_text(docker("logs", server, check=False))
        docker("rm", "-fv", server, check=False)
        for instance in instances:
            for kind in ("container", "network", "volume"):
                for resource in managed_resource(kind, instance):
                    if kind == "container":
                        (REPORT / f"managed-talo-worker-{resource}.log").write_text(docker("logs", resource, check=False))
                    docker(kind, "rm", *(["-f"] if kind == "container" else []), resource, check=False)
        if tag:
            docker("image", "rm", tag, check=False)
        docker("rm", "-fv", registry, check=False)


if __name__ == "__main__":
    tmp_root = Path(os.environ["TALOS_E2E_TMP_ROOT"])
    tmp_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="talo-e2e-", dir=tmp_root) as directory:
        run(Path(directory))
