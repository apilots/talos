"""Prepare pinned MCP-Atlas tasks and their per-case network and tool policies."""

import csv
import json
import os
from pathlib import Path
import shutil
import sys
from urllib.parse import urlsplit

from payload import resolve_model
from summarize import read_json, write_json

SMOKE = "barber-shop-first-word"
JUDGE_HOST = "api.openai.com"
JUDGE_MODEL = "gpt-4.1-2025-04-14"
SERVER_HOSTS = {
    "ddg-search": {"duckduckgo.com", "html.duckduckgo.com"},
    "open-library": {"openlibrary.org", "covers.openlibrary.org"},
    "osm-mcp-server": {
        "api.openstreetmap.org", "nominatim.openstreetmap.org",
        "overpass-api.de", "router.project-osrm.org",
    },
    "wikipedia": {"en.wikipedia.org"},
}
REFERENCE_HOSTS = {"6863f438a500a4b36aab0f24": {"engineersnetwork.org"}}


def model_connection(catalog):
    return resolve_model(
        catalog, None, os.environ["TALOS_BENCH_MODEL"],
        os.environ.get("TALOS_BENCH_CONFIG"), os.environ.get("TALOS_BENCH_DOTENV"),
    )


def prepare_public(case_dir, record, dataset):
    task_id = record["TASK"]
    enabled = json.loads(record["ENABLED_TOOLS"])
    tools = [tool if isinstance(tool, str) else tool["name"] for tool in enabled]
    servers = sorted({tool.split("_", 1)[0] for tool in tools})
    shutil.rmtree(case_dir / "solution")
    (case_dir / "instruction.md").write_text(record["PROMPT"] + "\n")
    (case_dir / "environment/enabled-tools.json").write_text(json.dumps(tools, indent=2) + "\n")
    write_json(case_dir / "upstream-task.json", record)

    groundtruth = case_dir / "tests/groundtruth.csv"
    with groundtruth.open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=["TASK", "PROMPT", "GTFA_CLAIMS"])
        writer.writeheader()
        writer.writerow({key: record[key] for key in writer.fieldnames})
    (case_dir / "tests/docker-compose.yaml").write_text(
        "services:\n  main:\n    platform: ${TALOS_BENCH_PLATFORM:?Set the native Docker platform}\n"
    )

    compose = case_dir / "environment/docker-compose.yaml"
    compose.write_text(compose.read_text().replace("filesystem,calculator", ",".join(servers)))

    judge_model = os.environ.get("TALOS_BENCH_ATLAS_JUDGE_MODEL", JUDGE_MODEL)
    task = case_dir / "task.toml"
    task.write_text(task.read_text()
        .replace(f"mcp-atlas/{SMOKE}", f"mcp-atlas/{task_id}")
        .replace("Controlled reproduction of the MCP-Atlas documentation example", "Published MCP-Atlas task")
        .replace('split = "controlled"', f'split = "public"\ndataset = "{dataset["dataset"]}"\n'
                 f'dataset_revision = "{dataset["revision"]}"')
        .replace("timeout_sec = 120.0", "timeout_sec = 1800.0")
        .replace("timeout_sec = 45.0", "timeout_sec = 420.0")
        .replace('environment_mode = "separate"', 'environment_mode = "separate"\n'
                 'network_mode = "allowlist"\n'
                 f'allowed_hosts = ["{JUDGE_HOST}"]\n\n'
                 '[verifier.env]\n'
                 'EVAL_LLM_API_KEY = "${TALOS_BENCH_ATLAS_JUDGE_API_KEY}"\n'
                 f'EVAL_LLM_BASE_URL = "https://{JUDGE_HOST}"\n'
                 f'EVAL_LLM_MODEL = {json.dumps(judge_model)}\n\n'
                 '[verifier.environment]\nworkdir = "/app"\ncpus = 1\nmemory_mb = 768\n'
                 'network_mode = "no-network"'))
    return set().union(
        REFERENCE_HOSTS.get(task_id, set()), *(SERVER_HOSTS.get(server, set()) for server in servers)
    )


def prepare(suite, destination, catalog, selection):
    connection, _ = model_connection(catalog)
    provider = connection["provider"]
    endpoint = urlsplit((connection.get("custom_providers") or connection)[provider]["base_url"])
    if endpoint.scheme != "https" or not endpoint.hostname:
        raise ValueError("The Atlas agent requires an HTTPS model endpoint")

    dataset = read_json(suite / "public-cases.json")
    records = {record["TASK"]: record for record in dataset["tasks"]}
    for task_id in selection:
        case_dir = destination / task_id
        shutil.copytree(suite / "cases" / SMOKE, case_dir, ignore=shutil.ignore_patterns("__pycache__"))
        hosts = {endpoint.hostname}
        if task_id != SMOKE:
            hosts.update(prepare_public(case_dir, records[task_id], dataset))
        if task_id != SMOKE:
            provenance = destination.parent / "provenance"
            provenance.mkdir(exist_ok=True)
            write_json(provenance / f"{task_id}.json", {
                "datasetRevision": dataset["revision"],
                "judgeModel": os.environ.get("TALOS_BENCH_ATLAS_JUDGE_MODEL", JUDGE_MODEL),
            })
        task = case_dir / "task.toml"
        task.write_text(task.read_text().replace(
            "[agent]\n", f'[agent]\nnetwork_mode = "allowlist"\nallowed_hosts = {json.dumps(sorted(hosts))}\n',
        ).replace("[environment]\n", '[environment]\nnetwork_mode = "no-network"\n'))
        compose = case_dir / "environment/docker-compose.yaml"
        compose.write_text(compose.read_text().removesuffix("networks:\n  default:\n    internal: true\n"))

    mcp = read_json(suite / "mcp.json")
    # Harbor's egress sidecar shares its network namespace with both services.
    mcp["mcpServers"]["atlas"]["env"]["ATLAS_URL"] = "http://127.0.0.1:1984"
    write_json(destination / "mcp.json", mcp)


if __name__ == "__main__":
    suite, destination, catalog, *selection = sys.argv[1:]
    prepare(Path(suite), Path(destination), Path(catalog), selection)
