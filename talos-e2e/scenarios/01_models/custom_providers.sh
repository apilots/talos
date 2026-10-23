#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
start_server
trap stop_server EXIT

case_dir="$(make_e2e_temp_dir custom-providers)"
initial_json="$(curl -fsS "${GATEWAY_URL}/v1/config/llm")"
api_data_json "${initial_json}" >"${case_dir}/initial.json"

python3 - "${case_dir}" <<'PY'
import json
from pathlib import Path
import sys

case_dir = Path(sys.argv[1])
config = json.loads((case_dir / "initial.json").read_text())["config"]
config["provider"] = "llm-gateway"
config["model"] = "model"
config["custom_providers"] = {}
for provider_id, name, route, thinking in [
    ("llm-gateway", "LLM Gateway", "gateway", True),
    ("local-inference", "Local Inference", "local", False),
]:
    config["custom_providers"][provider_id] = {
        "name": name,
        "api_key": None,
        "protocol": "chat",
        "base_url": f"http://127.0.0.1:9/{route}/v1",
        "models": [{
            "id": "model",
            "name": f"{name} Model",
            "context_window": 131072,
            "reasoning": True,
            "reasoning_effort": "medium",
            "reasoning_replay": "protocol_default",
            "speed": "fast" if thinking else "standard",
            "input": ["text"],
            "cost": {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0},
            "request_parameters": {
                "chat_template_kwargs": {"enable_thinking": thinking},
                "service_tier": "priority" if thinking else "default",
                "temperature": 0.2 if thinking else 0.7,
            },
        }],
    }
(case_dir / "update.json").write_text(json.dumps({"type": "llm", "value": config}))
PY

updated_json="$(curl -fsS -X PUT "${GATEWAY_URL}/v1/config" \
  -H "Content-Type: application/json" --data-binary "@${case_dir}/update.json")"
api_data_json "${updated_json}" >"${case_dir}/updated.json"
reloaded_json="$(curl -fsS -X POST "${GATEWAY_URL}/v1/config/reload")"
api_data_json "${reloaded_json}" >"${case_dir}/reloaded.json"
read_json="$(curl -fsS "${GATEWAY_URL}/v1/config/llm")"
api_data_json "${read_json}" >"${case_dir}/read.json"
models_json="$(curl -fsS "${GATEWAY_URL}/v1/models")"
api_data_json "${models_json}" >"${case_dir}/models.json"

python3 - "${case_dir}" "${TALOS_HOME}/conf/talos.yaml" <<'PY'
import json
from pathlib import Path
import sys
import yaml

case_dir = Path(sys.argv[1])
read = lambda name: json.loads((case_dir / f"{name}.json").read_text())
expected = read("update")["value"]
initial = read("initial")["config"]
assert read("reloaded")["success"] is True, "configuration reload failed"
for stage in ["updated", "read"]:
    response = read(stage)
    config = response["config"]
    assert config["custom_providers"] == expected["custom_providers"], stage
    assert (config["provider"], config["model"]) == ("llm-gateway", "model"), stage
    assert response["model_readiness"]["ready"] is True, response["model_readiness"]
    for provider_id in expected["custom_providers"]:
        assert "chat" in response["provider_capabilities"][provider_id]["protocols"]
    for provider_id in ["anthropic", "openai", "bailian", "deepseek", "ollama", "openrouter"]:
        assert config[provider_id] == initial[provider_id], f"modified built-in {provider_id}"

persisted = yaml.safe_load(Path(sys.argv[2]).read_text())["llm"]
assert persisted["custom_providers"] == expected["custom_providers"], "persistence mismatch"
model_response = read("models")
catalog = model_response["models"]
custom_models = [model for model in catalog if model["provider"] in expected["custom_providers"]]
assert len(custom_models) == 2, custom_models
assert {(model["provider"], model["id"]) for model in custom_models} == {
    ("llm-gateway", "model"), ("local-inference", "model"),
}, "same upstream model ID lost provider identity"
for model in custom_models:
    provider = expected["custom_providers"][model["provider"]]
    assert model["provider_name"] == provider["name"], model
    for key, value in provider["models"][0].items():
        assert model[key] == value, (model["provider"], key)
    thinking = model["request_parameters"]["chat_template_kwargs"]["enable_thinking"]
    assert model["selection_capabilities"] == {
        "reasoning": "toggle",
        "default_reasoning_effort": "medium" if thinking else None,
        "fast": True,
        "default_fast": thinking,
    }, (model["provider"], model["selection_capabilities"])
    print(f"CASE custom_provider_catalog PASS provider={model['provider']} name={model['provider_name']} model={model['id']}")
assert model_response["defaultModelSelection"] == {
    "modelRef": {"provider": "llm-gateway", "id": "model"},
    "reasoningEffort": "medium",
}, model_response["defaultModelSelection"]
print("CASE configured_model_selection_capabilities PASS thinking=toggle fast=explicit defaults=canonical")
print("CASE custom_provider_persistence PASS save=read=reload=disk builtins=unchanged optional_key=ready")
PY
