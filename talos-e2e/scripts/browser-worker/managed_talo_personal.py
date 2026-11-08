"""Exercise registration-only removal and explicit personal Worker re-enrollment."""
import json
import time
import uuid

import yaml

from fixture import REPORT, ROOT, data, http, http_response, wait
from lifecycle import docker_create
from media import Viewer


def create_computer(base):
    data(http(base + "/v1/user"))
    return data(http(base + "/v1/computers", {
        "displayName": "Disposable personal Worker",
        "credentialExpiresAt": int(time.time()) + 3600,
    }))


def remove_computer(base, node):
    url = base + "/v1/computers/" + node
    status, response = http_response(url, method="DELETE")
    assert status == 200 and data(response)["nodeId"] == node
    assert http(url)["status"] != "success"


def wait_available(base, node):
    def available():
        response = http(base + "/v1/computers/" + node)
        return response.get("data", {}).get("available", False)

    wait(available, "personal Worker registration")


def verify_personal_capabilities(base, node):
    session = "sess_personal_" + uuid.uuid4().hex
    data(http(base + "/v1/turns/start", {
        "sessionId": session,
        "nodeId": node,
        "modelSelection": {"modelRef": {"provider": "openai", "id": "browser-fixture"}, "reasoningEffort": None},
        "input": {"content": [{"type": "text", "text": "Personal Worker acceptance Turn"}]},
    }))

    def terminal_turn():
        turns = data(http(base + f"/v1/sessions/{session}/history"))["data"]
        return next((turn["result"] for turn in turns
                     if turn["result"]["status"] in {"completed", "failed", "cancelled"}), None)

    result = wait(terminal_turn, "personal Worker's local Provider Turn")
    assert result["status"] == "completed" and "TALO_WAKE_OK" in json.dumps(result)
    state = data(http(base + "/v1/browser", {"nodeId": node, "operation": {"kind": "read"}}))
    grant = data(http(base + "/v1/desktop/open", {
        "nodeId": node, "targetId": "managed", "viewerId": str(uuid.uuid4()),
        "generation": state["control"]["generation"], "inputMode": "readOnly",
        "controllerId": None, "inputEpoch": None,
    }))
    viewer = Viewer(base, grant)
    try:
        viewer.frame(REPORT / "personal-worker-frame.png")
    finally:
        viewer.close()
    print("PASS personal Worker executes its local Provider Turn and renders real desktop pixels", flush=True)


def docker_personal(files, base, worker_model, image, docker, container_args):
    config = yaml.safe_load((files / "talos.yaml").read_text())
    endpoint = config.pop("talos")["worker"]["runtime"]["control_plane_endpoint"]
    model_url = worker_model["openai"]["base_url"]
    config["llm"] = worker_model
    config["cloud"].update(api_base_url=model_url.removesuffix("/v1"), agent_base_url=endpoint,
                           agent_trusted_ca_file="/home/talos/.talos/conf/tls.crt")
    name = "talo-parity-personal-" + uuid.uuid4().hex[:8]
    volume = name + "-home"
    mount = f"{volume}:/home/talos/.talos"
    seed = files / "personal-worker"
    seed.mkdir()
    (seed / "tls.crt").write_bytes((files / "server.crt").read_bytes())

    def provision():
        created = create_computer(base)
        node = created["node"]["nodeId"]
        config["agent_node"] = {"runner": {
            "node_id": node,
            "control_plane_endpoint": endpoint,
            "credential_file": "/home/talos/.talos/conf/node.credential",
            "trusted_ca_file": "/home/talos/.talos/conf/tls.crt",
            "transport": "outbound",
        }}
        (seed / "talos.yaml").write_text(yaml.safe_dump(config))
        credential = seed / "node.credential"
        credential.touch(mode=0o600)
        credential.write_text(created["credential"]["token"])
        docker("run", "--rm", *container_args(name + "-seed"), "--user", "0", "-v", mount,
               "-v", f"{seed}:/fixture:ro", image, "/bin/bash", "-ec",
               "mkdir -p /home/talos/.talos/conf; cp /fixture/* /home/talos/.talos/conf/; "
               "chown -R 10001:10001 /home/talos/.talos; chmod 600 /home/talos/.talos/conf/node.credential")
        return node

    try:
        docker_create("volume", volume, [])
        old_node = provision()
        container = docker("run", "-d", *container_args(name), "--shm-size", "1g",
                           "--security-opt", "no-new-privileges:true", "--security-opt",
                           "seccomp=" + str(ROOT / "deploy/docker/talos-worker/chromium-seccomp.json"),
                           "-v", mount, "-e", "TALOS_CONFIG=/home/talos/.talos/conf/talos.yaml", image)
        wait_available(base, old_node)
        verify_personal_capabilities(base, old_node)
        docker("exec", name, "/bin/bash", "-ec", 'printf retained > /home/talos/.talos/parity-retained')
        remove_computer(base, old_node)
        assert docker("inspect", "--format", "{{.Id}}", name) == container
        assert docker("volume", "inspect", "--format", "{{.Name}}", volume) == volume
        state = docker("inspect", "--format", "{{.State.Status}}", name)
        docker("stop", name, check=False)
        new_node = provision()
        assert new_node != old_node
        docker("start", name)
        wait_available(base, new_node)
        assert docker("inspect", "--format", "{{.Id}}", name) == container
        assert docker("exec", name, "cat", "/home/talos/.talos/parity-retained") == "retained"
        (REPORT / "personal-docker-registration.json").write_text(json.dumps({
            "registrationRemoved": True, "containerPreserved": True, "volumePreserved": True,
            "stateAfterRemoval": state, "explicitReRegistration": True, "homeFileRetained": True,
        }, indent=2))
        print("PASS personal Worker registration removal preserves Docker container and Home", flush=True)
    finally:
        docker("rm", "-fv", name, check=False)
        docker("volume", "rm", volume, check=False)
