"""Export a completed Atlas trial for the upstream scorer."""

import argparse
import csv
import json
from pathlib import Path
import re
import tomllib


def recorded_messages(transcript):
    messages = []
    for block in transcript:
        content = block["content"]
        kind, data = content["type"], content["data"]
        if kind == "userContent":
            messages.append({"role": "user", "content": data["text"]})
        elif kind == "toolResult":
            messages.append({
                "role": "tool",
                "tool_call_id": data["callId"],
                "content": json.dumps(data["content"], ensure_ascii=False),
            })
        elif kind in {"assistantText", "toolCall"}:
            if not messages or messages[-1]["role"] != "assistant":
                messages.append({"role": "assistant", "content": None})
            message = messages[-1]
            if kind == "assistantText":
                message["content"] = (message["content"] or "") + data["text"]
            else:
                arguments = data["arguments"]
                serialized = (json.dumps(arguments["value"], ensure_ascii=False)
                              if "value" in arguments else arguments["jsonFragment"])
                message.setdefault("tool_calls", []).append({
                    "id": data["callId"],
                    "type": "function",
                    "function": {"name": data["name"], "arguments": serialized},
                })
    return messages


def export(trial, output):
    agent = trial / "agent"
    verifier = trial / "verifier"
    result = json.loads((trial / "result.json").read_text())
    run = json.loads((agent / "talos-run.json").read_text())
    with (verifier / "candidate.csv").open(newline="") as stream:
        candidate, = csv.DictReader(stream)
    task_id = candidate["task_id"]
    if result["task_name"] != f"mcp-atlas/{task_id}" or result["exception_info"] or run["exit_code"] != 0:
        raise ValueError("Expected a completed MCP-Atlas trial")
    transcript = json.loads((agent / "talos-transcript.json").read_text())
    history = recorded_messages(transcript)
    if not history:
        raise ValueError("The Atlas trial is missing recorded conversation history")
    task = trial.parents[1] / "task" / task_id
    if not task.is_dir():
        task = Path(result["config"]["task"]["path"])
    metadata = tomllib.loads((task / "task.toml").read_text())["metadata"]
    image = re.search(r"(?m)^    image: (\S+)$", (task / "environment/docker-compose.yaml").read_text())[1]
    scores = {name: (verifier / name).read_bytes() for name in (
        "scored_talos.csv", "coverage_stats_talos_all.json",
        "coverage_stats_talos_combined.json", "judge-verdicts.json",
    )}
    stats = json.loads(scores["coverage_stats_talos_all.json"])
    config = {
        "model": run["model"].replace(":", "/", 1),
        "reasoning_effort": run.get("reasoning_effort"),
        "num_tasks": 1,
        "image": image,
        "judge": stats["evaluator_model"],
        "split": metadata["split"],
        "scorer_revision": metadata["upstream_revision"],
        "task_checksum": result["task_checksum"],
    }
    if metadata["split"] == "public":
        config.update(dataset=metadata["dataset"], dataset_revision=metadata["dataset_revision"])

    output.mkdir(parents=True)
    with (output / "outputs.csv").open("w", newline="") as stream:
        writer = csv.writer(stream)
        writer.writerow(("task_id", "raw_conversation_history", "response"))
        writer.writerow((task_id, json.dumps(history, ensure_ascii=False), candidate["response"]))
    (output / "run_config.json").write_text(json.dumps(config, indent=2) + "\n")
    for name, content in scores.items():
        (output / name).write_bytes(content)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trial", type=Path)
    parser.add_argument("output", type=Path)
    export(**vars(parser.parse_args()))
