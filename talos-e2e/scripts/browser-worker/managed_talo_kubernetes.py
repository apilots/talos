"""Run the shared managed Talo journey against the checked-in single-node k3d deployment."""
import argparse
import base64
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

import yaml

from fixture import ACCOUNTS, REPORT, ROOT, TOKEN, configure, data, free_port, http, http_response, wait
from managed_talo import FIXTURE_PROGRAM, LABEL, RECOVERY, SERVER_IMAGE, WORKER_IMAGE, exercise
from managed_talo_personal import create_computer, remove_computer, verify_personal_capabilities, wait_available

DEPLOY = ROOT / "deploy/kubernetes"
SERVICE = "talos-server"
HOST = "talos-server.talos.svc"
STORAGE_CLASS = "talos-xfs-quota"
ROLE = "io.talos.managed-worker.resource"
PERSONAL = "talos-personal-worker"


def command(*args, input_text=None, check=True):
    return subprocess.run(args, input=input_text, check=check, capture_output=True,
                          text=True, timeout=300).stdout.strip()


class KubeObservations:
    disk_quota = "enforced"
    start_reuses_compute = False

    def __init__(self, fixture):
        self.fixture = fixture
        self.volumes = {}

    def select(self, resource, instance, role):
        return self.fixture.items(resource, f"{LABEL}={instance},{ROLE}={role}")

    def compute(self, instance):
        [pod] = self.select("pods", instance, "worker")
        return pod["metadata"]["name"]

    def identity(self, pod):
        return self.fixture.read("pod", pod)["metadata"]["uid"]

    def replacement(self, instance, previous_id):
        return next((pod["metadata"]["name"] for pod in self.select("pods", instance, "worker")
                     if pod["metadata"]["uid"] != previous_id), None)

    def home(self, instance):
        [claim] = self.fixture.items("pvc", f"{LABEL}={instance}")
        assert claim["spec"]["resources"]["requests"]["storage"] == "100G", claim["spec"]
        assert claim["spec"]["storageClassName"] == STORAGE_CLASS
        pv = claim["spec"]["volumeName"]
        assert self.fixture.read("pv", pv)["spec"]["persistentVolumeReclaimPolicy"] == "Delete"
        self.volumes[instance] = pv
        return claim["metadata"]["uid"]

    def stopped(self, instance, _pod):
        [workload] = self.select("statefulsets", instance, "worker")
        return workload["spec"]["replicas"] == 0 and not self.select("pods", instance, "worker")

    def running(self, instance):
        return any(pod["status"]["phase"] == "Running" for pod in self.select("pods", instance, "worker"))

    def execute(self, pod, *args):
        return self.fixture.kube("exec", pod, "--", *args)

    def assert_removed(self, instance):
        for resource in ("statefulsets", "pods", "pvc", "jobs", "secrets"):
            assert not self.fixture.items(resource, f"{LABEL}={instance}"), resource
        self.fixture.kube("wait", "--for=delete", f"pv/{self.volumes[instance]}", "--timeout=120s")


class Fixture:
    def __init__(self, files):
        self.files = files
        self.cluster = os.environ["TALOS_E2E_K3D_CLUSTER"]
        self.context = f"k3d-{self.cluster}"
        self.kubectl = ("kubectl", "--context", self.context, "--namespace", "talos")
        self.base = f"http://127.0.0.1:{free_port()}"
        self.faults = f"http://127.0.0.1:{free_port()}"
        self.instances = []
        self.created = []
        self.forward_process = None
        self.observed = KubeObservations(self)

    def kube(self, *args, **kwargs):
        try:
            return command(*self.kubectl, *args, **kwargs)
        except subprocess.CalledProcessError as error:
            raise RuntimeError(f"kubectl {args[0]} failed: {error.stderr.strip()}") from error

    def read(self, kind, name):
        return json.loads(self.kube("get", kind, name, "-o", "json"))

    def items(self, kind, labels):
        return json.loads(self.kube("get", kind, "-l", labels, "-o", "json"))["items"]

    def create(self, *args, input_text=None):
        try:
            output = self.kube("create", *args, "-o", "name", input_text=input_text)
        except RuntimeError as error:
            self.created.extend(error.__cause__.stdout.strip().splitlines())
            raise
        self.created.extend(output.splitlines())

    def guard_names(self):
        groups = [
            ("deployment", SERVICE, PERSONAL),
            ("pvc", SERVICE + "-home", PERSONAL + "-home"),
            ("service", SERVICE),
            ("serviceaccount", SERVICE),
            ("role", SERVICE),
            ("rolebinding", SERVICE),
            ("configmap", SERVICE + "-bootstrap", "talos-e2e-fixture"),
            ("secret", SERVICE + "-tls", PERSONAL + "-bootstrap", "talos-e2e-kubeconfig"),
            ("networkpolicy", "talos-seeder-isolation", "talos-worker-no-ingress"),
        ]
        for group in groups:
            found = self.kube("get", *group, "--ignore-not-found", "-o", "name")
            if found:
                raise RuntimeError(f"Local acceptance requires disposable names in talos; found {found}")

    def make_tls(self):
        cert = self.files / "tls.crt"
        key = self.files / "tls.key"
        command("openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                "-subj", f"/CN={HOST}", "-addext", f"subjectAltName=DNS:{HOST}",
                "-addext", "basicConstraints=critical,CA:FALSE",
                "-addext", "keyUsage=critical,digitalSignature,keyEncipherment",
                "-addext", "extendedKeyUsage=serverAuth", "-keyout", str(key), "-out", str(cert))
        return cert, key

    def deploy(self, preflight):
        self.guard_names()
        cert, key = self.make_tls()
        importer = DEPLOY / "overlays/k3d/import-image.sh"
        server_image = command(str(importer), self.cluster, SERVER_IMAGE, "server")
        self.worker_image = command(str(importer), self.cluster, WORKER_IMAGE, "worker")
        profile = configure(yaml.safe_load((ROOT / "conf/talos-dev.yaml").read_text()), self.files, 8080, 18080)
        profile["gateway"]["host"] = "0.0.0.0"
        profile["agent_node"] = {"control_plane": {
            "bind_address": "0.0.0.0:9443",
            "tls_certificate_file": "/run/talos-tls/tls.crt",
            "tls_private_key_file": "/run/talos-tls/tls.key",
            "trusted_node_ca_file": "/run/talos-tls/ca.crt",
        }}
        profile["cloud"].update(agent_base_url=f"https://{HOST}:9443", agent_trusted_ca_file="/run/talos-tls/ca.crt")
        profile["talos"] = {
            "worker": {"runtime": {
                "kind": "Kubernetes", "image": self.worker_image,
                "control_plane_endpoint": f"https://{HOST}:9443",
                "trusted_ca_file": "/run/talos-tls/ca.crt",
                "kubernetes": {
                    "namespace": "talos", "storage_class": STORAGE_CLASS,
                    "seccomp_profile": "talos/chromium-seccomp.json", "home_quota_enforced": True,
                },
            }},
            "server": {"runtime": {"idle_timeout_seconds": 12}},
        }
        if preflight:
            profile["talos"]["worker"]["runtime"]["kubernetes"].update(
                kubeconfig_source="InCluster", kubeconfig_path="/var/run/secrets/talos-kubernetes/kubeconfig",
            )
        config = self.files / "talos.yaml"
        config.write_text(yaml.safe_dump(profile, sort_keys=False))
        self.worker_model = profile["llm"]
        self.worker_model["openai"]["base_url"] = f"http://{HOST}:18080/v1"
        self.create("configmap", SERVICE + "-bootstrap", f"--from-file={config}")
        self.create("secret", "generic", SERVICE + "-tls", "--type=kubernetes.io/tls",
                    f"--from-file=tls.crt={cert}", f"--from-file=tls.key={key}", f"--from-file=ca.crt={cert}")
        self.deploy_tree = self.files / "deployment"
        shutil.copytree(DEPLOY, self.deploy_tree)
        overlay = self.deploy_tree / "overlays/k3d"
        self.add_fixture(overlay, server_image, preflight)
        self.create("-k", str(overlay))
        self.rollout(SERVICE)

    def add_fixture(self, overlay, server_image, preflight):
        for source in ("fixture.py", "../lifecycle.py"):
            source = Path(__file__).parent / source
            shutil.copyfile(source, overlay / source.name)
        (overlay / "fixture_program.py").write_text(FIXTURE_PROGRAM)
        kustomization = overlay / "kustomization.yaml"
        doc = yaml.safe_load(kustomization.read_text())
        repository, digest = server_image.split("@")
        doc["images"] = [{"name": "ghcr.io/apilots/talos-server", "newName": repository, "digest": digest}]
        doc["configMapGenerator"] = [{
            "name": "talos-e2e-fixture", "files": ["fixture.py", "fixture_program.py", "lifecycle.py"],
        }]
        doc["generatorOptions"] = {"disableNameSuffixHash": True}
        doc["namespace"] = "talos"
        doc["patches"].extend([
            {"target": {"kind": "Deployment", "name": SERVICE}, "patch": yaml.safe_dump({
                "apiVersion": "apps/v1", "kind": "Deployment", "metadata": {"name": SERVICE},
                "spec": {"template": {"spec": {
                    "volumes": [{"name": "e2e-fixture", "configMap": {"name": "talos-e2e-fixture"}}],
                    "containers": [{
                        "name": "fixture", "image": server_image,
                        "command": ["/usr/bin/tini", "--", "python3", "/fixture/program/fixture_program.py"],
                        "securityContext": {"allowPrivilegeEscalation": False, "capabilities": {"drop": ["ALL"]}},
                        "env": [
                            {"name": "TALOS_E2E_FIXTURE_SIDECAR", "value": "1"},
                            {"name": "TALOS_E2E_ADMIN_ACCOUNT", "value": "1"},
                            {"name": "TALOS_E2E_TALO_RECOVERY", "value": "1" if RECOVERY else "0"},
                            {"name": "TALOS_E2E_TMP_ROOT", "value": "/tmp/talo-e2e"},
                            {"name": "TALOS_SCENARIO_REPORT_DIR", "value": "/tmp/talo-e2e"},
                        ],
                        "ports": [{"name": "fixture-http", "containerPort": 18080},
                                  {"name": "fixture-tls", "containerPort": 9444}],
                        "volumeMounts": [{"name": "e2e-fixture", "mountPath": "/fixture/program", "readOnly": True}],
                    }],
                }}},
            })},
            {"target": {"kind": "Service", "name": SERVICE}, "patch": yaml.safe_dump([
                {"op": "add", "path": "/spec/ports/-", "value":
                    {"name": "fixture-http", "port": 18080, "targetPort": "fixture-http"}},
                *([{"op": "replace", "path": "/spec/ports/1/targetPort", "value": "fixture-tls"}] if RECOVERY else []),
            ])},
        ])
        if preflight:
            doc["patches"].append({"target": {"kind": "Deployment", "name": SERVICE}, "patch": yaml.safe_dump({
                "apiVersion": "apps/v1", "kind": "Deployment", "metadata": {"name": SERVICE},
                "spec": {"template": {"spec": {
                    "volumes": [{"name": "operator-kubeconfig", "secret": {
                        "secretName": "talos-e2e-kubeconfig", "optional": True, "defaultMode": 0o440,
                    }}],
                    "containers": [{"name": "server", "volumeMounts": [{
                        "name": "operator-kubeconfig", "mountPath": "/var/run/secrets/talos-kubernetes", "readOnly": True,
                    }]}],
                }}},
            })})
        kustomization.write_text(yaml.safe_dump(doc, sort_keys=False))

    def rollout(self, deployment):
        self.kube("rollout", "status", "deployment/" + deployment, "--timeout=180s")

    def start_forward(self):
        ports = [f"{self.base.rsplit(':', 1)[1]}:8080", f"{self.faults.rsplit(':', 1)[1]}:18080"]
        with (REPORT / "kubernetes-port-forward.log").open("a") as log:
            self.forward_process = subprocess.Popen([*self.kubectl, "port-forward", "--address", "127.0.0.1",
                                                     f"service/{SERVICE}", *ports], stdout=log, stderr=log)
        wait(lambda: self.forward_process.poll() is None and http(self.base + "/health"), "k3d Server port-forward")

    def stop_forward(self):
        process = self.forward_process
        if process is not None and process.poll() is None:
            process.terminate()
            process.wait(timeout=10)

    @contextmanager
    def forward(self):
        try:
            self.start_forward()
            yield
        finally:
            self.stop_forward()

    def restart(self):
        self.stop_forward()
        self.kube("rollout", "restart", "deployment/" + SERVICE)
        self.rollout(SERVICE)
        self.start_forward()

    def set_personal_credentials(self):
        created = create_computer(self.base)
        node = created["node"]["nodeId"]
        profile = yaml.safe_load((DEPLOY / "manual-worker/talos.yaml.example").read_text())
        profile["llm"] = self.worker_model
        config = self.files / "personal.yaml"
        config.write_text(yaml.safe_dump(profile))
        credential = self.files / "node.credential"
        credential.touch(mode=0o600)
        credential.write_text(created["credential"]["token"])
        node_id = self.files / "node-id"
        node_id.write_text(node)
        args = ("secret", "generic", PERSONAL + "-bootstrap", f"--from-file=node-id={node_id}",
                f"--from-file=talos.yaml={config}", f"--from-file={credential}",
                f"--from-file=ca.crt={self.files / 'tls.crt'}")
        if "secret/" + PERSONAL + "-bootstrap" in self.created:
            desired = self.kube("create", *args, "--dry-run=client", "-o", "yaml")
            secret = yaml.safe_load(desired)
            secret["metadata"]["resourceVersion"] = self.read("secret", PERSONAL + "-bootstrap")["metadata"]["resourceVersion"]
            self.kube("replace", "-f", "-", input_text=yaml.safe_dump(secret))
        else:
            self.create(*args)
        return node

    def personal(self):
        old_node = self.set_personal_credentials()
        overlay = self.deploy_tree / "manual-worker"
        kustomization = overlay / "kustomization.yaml"
        doc = yaml.safe_load(kustomization.read_text())
        repository, digest = self.worker_image.split("@")
        doc["images"] = [{"name": "ghcr.io/apilots/talos-worker", "newName": repository, "digest": digest}]
        kustomization.write_text(yaml.safe_dump(doc))
        self.create("-k", str(overlay))
        self.rollout(PERSONAL)
        wait_available(self.base, old_node)
        verify_personal_capabilities(self.base, old_node)
        workload = self.read("deployment", PERSONAL)["metadata"]["uid"]
        home = self.read("pvc", PERSONAL + "-home")["metadata"]["uid"]
        self.kube("exec", "deployment/" + PERSONAL, "--", "/bin/bash", "-ec", 'printf retained > "$TALOS_HOME/parity-retained"')
        remove_computer(self.base, old_node)
        assert self.read("deployment", PERSONAL)["metadata"]["uid"] == workload
        assert self.read("pvc", PERSONAL + "-home")["metadata"]["uid"] == home
        new_node = self.set_personal_credentials()
        assert new_node != old_node
        self.kube("rollout", "restart", "deployment/" + PERSONAL)
        self.rollout(PERSONAL)
        wait_available(self.base, new_node)
        assert self.read("deployment", PERSONAL)["metadata"]["uid"] == workload
        assert self.read("pvc", PERSONAL + "-home")["metadata"]["uid"] == home
        assert self.kube("exec", "deployment/" + PERSONAL, "--", "cat", "/home/talos/.talos/parity-retained") == "retained"
        print("PASS personal Worker registration removal preserves Deployment and PVC; re-registration retains Home", flush=True)

    def cleanup(self):
        if "deployment.apps/" + SERVICE in self.created:
            for name in (SERVICE, PERSONAL):
                log = self.kube("logs", "deployment/" + name, "--all-containers", "--tail=300", check=False)
                (REPORT / f"{name}-kubernetes.log").write_text(log)
        for instance in self.instances:
            for pod in self.items("pods", f"{LABEL}={instance}"):
                name = pod["metadata"]["name"]
                log = self.kube("logs", name, "--all-containers", "--tail=400", check=False)
                (REPORT / f"{name}.log").write_text(log)
            self.kube("delete", "statefulsets,pods,jobs,secrets,pvc", "-l", f"{LABEL}={instance}",
                      "--ignore-not-found", "--timeout=120s")
        workloads = [name for name in self.created if name.startswith("deployment.apps/")]
        if workloads:
            self.kube("delete", *workloads, "--ignore-not-found", "--timeout=120s")
        resources = [name for name in self.created if name not in workloads]
        if resources:
            self.kube("delete", *resources, "--ignore-not-found", "--timeout=120s")


def runtime_request(base, token, method="GET", payload=None, validate=False):
    endpoint = "/v1/worker-runtime" + ("/validate" if validate else "")
    return http_response(base + endpoint, payload, token, method=method)


def run_preflight(fixture):
    for filename, script in [
        ("kubernetes-runtime-rbac.log", DEPLOY / "check-runtime-access.sh"),
        ("kubernetes-storage-quota.log", DEPLOY / "storage/check-quota.sh"),
    ]:
        with (REPORT / filename).open("w") as log:
            subprocess.run([script, fixture.context, STORAGE_CLASS], stdout=log, stderr=subprocess.STDOUT,
                           timeout=420, check=True)
        print((REPORT / filename).read_text(), end="", flush=True)

    isolation = fixture.read("networkpolicy", "talos-worker-no-ingress")["spec"]
    assert isolation["policyTypes"] == ["Ingress"] and not isolation.get("ingress")
    assert isolation["podSelector"]["matchLabels"] == {ROLE: "worker"}
    seed = fixture.read("networkpolicy", "talos-seeder-isolation")["spec"]
    assert set(seed["policyTypes"]) == {"Ingress", "Egress"}
    assert seed["podSelector"]["matchLabels"] == {ROLE: "seeder"}
    assert not seed.get("ingress") and not seed.get("egress")
    home = fixture.read("pvc", SERVICE + "-home")["metadata"]["uid"]
    token = fixture.kube("create", "token", SERVICE, "--duration=20m")
    ca = fixture.read("configmap", "kube-root-ca.crt")["data"]["ca.crt"]
    kubeconfig = yaml.safe_dump({
        "apiVersion": "v1", "kind": "Config", "current-context": "talos-e2e",
        "clusters": [{"name": "cluster", "cluster": {"server": "https://kubernetes.default.svc",
                      "certificate-authority-data": base64.b64encode(ca.encode()).decode()}}],
        "contexts": [{"name": "talos-e2e", "context": {"cluster": "cluster", "user": "server", "namespace": "talos"}}],
        "users": [{"name": "server", "user": {"token": token}}],
    })
    mounted = fixture.files / "operator-kubeconfig"
    mounted.touch(mode=0o600)
    mounted.write_text(kubeconfig)
    fixture.create("secret", "generic", "talos-e2e-kubeconfig", f"--from-file=kubeconfig={mounted}")
    fixture.kube("rollout", "restart", f"deployment/{SERVICE}")
    fixture.rollout(SERVICE)

    bob = next(account for account in ACCOUNTS if account != TOKEN)
    candidate = {"kind": "kubernetes", "namespace": "talos", "storageClass": STORAGE_CLASS}
    in_cluster = {**candidate, "authentication": {"kind": "inCluster"}}
    operator_mounted = {**candidate, "authentication": {"kind": "mounted", "context": "talos-e2e"}}
    pasted = {**candidate, "authentication": {"kind": "pasted", "context": "talos-e2e", "kubeconfig": kubeconfig}}
    with fixture.forward():
        for account in ACCOUNTS:
            data(http(fixture.base + "/v1/user", token=account))
        assert runtime_request(fixture.base, bob)[0] == 403
        assert runtime_request(fixture.base, bob, "POST", in_cluster, validate=True)[0] == 403
        assert runtime_request(fixture.base, bob, "PUT", in_cluster)[0] == 403
        status, response = runtime_request(fixture.base, TOKEN)
        active = data(response)
        assert status == 200 and active["activeBackend"] == "kubernetes" and not active["restartRequired"]
        assert active["kubernetes"]["authenticationSource"] == "inCluster"
        for valid in (in_cluster, operator_mounted, pasted):
            status, response = runtime_request(fixture.base, TOKEN, "POST", valid, validate=True)
            assert status == 200 and data(response) is None, response
            assert token not in json.dumps(response)
        invalid = {**candidate, "authentication": {"kind": "pasted", "kubeconfig": "invalid"}}
        status, response = runtime_request(fixture.base, TOKEN, "PUT", invalid)
        assert 400 <= status < 500 and token not in json.dumps(response)
        assert data(runtime_request(fixture.base, TOKEN)[1]) == active
        status, response = runtime_request(fixture.base, TOKEN, "PUT", pasted)
        assert status == 200 and token not in json.dumps(response)
        pending = data(response)
        assert pending["restartRequired"] and pending["kubernetes"]["authenticationSource"] == "pasted"

    fixture.kube("rollout", "restart", f"deployment/{SERVICE}")
    fixture.rollout(SERVICE)
    with fixture.forward():
        data(http(fixture.base + "/v1/user"))
        status, response = runtime_request(fixture.base, TOKEN)
        saved = data(response)
        assert status == 200 and saved["activeBackend"] == "kubernetes" and not saved["restartRequired"]
        assert saved["kubernetes"]["authenticationSource"] == "pasted"
        assert token not in json.dumps(response)
    assert fixture.read("pvc", SERVICE + "-home")["metadata"]["uid"] == home
    persisted = fixture.kube("exec", "deployment/" + SERVICE, "-c", "server", "--", "cat",
                             "/home/talos/.talos/conf/talos.yaml")
    assert token not in persisted
    ref = yaml.safe_load(persisted)["talos"]["worker"]["runtime"]["kubernetes"]["pasted_kubeconfig"]
    credential_directory = "/home/talos/.talos/data/worker-runtime-credentials"
    permissions = fixture.kube("exec", "deployment/" + SERVICE, "-c", "server", "--",
                               "stat", "-c", "%a", credential_directory, f"{credential_directory}/{ref}.yaml")
    assert permissions.splitlines() == ["700", "600"]
    (REPORT / "kubernetes-runtime-settings.json").write_text(json.dumps(saved, indent=2))
    print("PASS Runtime admin gating, three authentication sources, invalid save isolation and restart persistence", flush=True)


def run(files, preflight):
    fixture = Fixture(files)
    try:
        fixture.deploy(preflight)
        if preflight:
            run_preflight(fixture)
        else:
            with fixture.forward():
                if not RECOVERY:
                    fixture.personal()
                exercise(fixture.base, fixture.instances, fixture.worker_model, fixture.faults, fixture.observed,
                         None if RECOVERY else fixture.restart)
    finally:
        fixture.cleanup()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--preflight", action="store_true")
    args = parser.parse_args()
    temp_root = Path(os.environ["TALOS_E2E_TMP_ROOT"])
    temp_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="talo-k3d-", dir=temp_root) as directory:
        run(Path(directory), args.preflight)
