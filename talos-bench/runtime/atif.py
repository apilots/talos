"""Project the opt-in Talos execution transcript into Harbor ATIF."""

from datetime import datetime, timezone
import json

from harbor.models.trajectories import (
    Agent, FinalMetrics, Observation, ObservationResult, Step, ToolCall, Trajectory,
)
from harbor.utils.trajectory_validator import TrajectoryValidator

from payload import cli_log_lines, run_metrics


def export_atif(logs):
    run = json.loads((logs / "talos-run.json").read_text())
    if run["exit_code"]:
        return
    trace = json.loads((logs / "talos-trajectory.json").read_text())
    transcript = json.loads((logs / "talos-transcript.json").read_text())
    steps = []
    calls = {}
    current = None

    for block in transcript:
        kind = block["content"]["type"]
        data = block["content"]["data"]
        if kind == "toolResult":
            step = calls[data["callId"]]
            if step.observation is None:
                step.observation = Observation(results=[])
            step.observation.results.append(ObservationResult(
                source_call_id=data["callId"],
                content=json.dumps(data["content"], ensure_ascii=False),
                extra={"is_error": data["isError"]},
            ))
            continue

        timestamp = datetime.fromtimestamp(block["startedAt"] / 1000, timezone.utc).isoformat()
        if kind == "userContent":
            steps.append(Step(step_id=len(steps) + 1, source="user", timestamp=timestamp,
                              message=data["text"]))
            current = None
            continue

        if current is None or current.observation is not None:
            current = Step(step_id=len(steps) + 1, source="agent", timestamp=timestamp, message="")
            steps.append(current)
        if kind == "assistantText":
            current.message += data["text"]
        elif kind == "reasoning":
            current.reasoning_content = (current.reasoning_content or "") + data["text"]
        elif kind == "toolCall":
            arguments = data["arguments"]
            value = arguments.get("value", arguments.get("jsonFragment"))
            call = ToolCall(
                tool_call_id=data["callId"],
                function_name=data["name"],
                arguments=value if isinstance(value, dict) else {},
                extra={"talos_arguments": arguments} if not isinstance(value, dict) else None,
            )
            if current.tool_calls is None:
                current.tool_calls = []
            current.tool_calls.append(call)
            calls[data["callId"]] = current

    if not steps or steps[0].source != "user" or not any(step.source == "agent" for step in steps):
        raise ValueError("The Talos transcript must contain user and agent content")
    usage = (run_metrics(cli_log_lines(logs)) or {}).get("usage", {})
    trajectory = Trajectory(
        schema_version="ATIF-v1.8",
        session_id=trace["identity"]["sessionId"],
        agent=Agent(name="talos", version=run["version"], model_name=run["model"].replace(":", "/", 1)),
        steps=steps,
        final_metrics=FinalMetrics(
            total_steps=len(steps),
            total_prompt_tokens=usage.get("input_tokens"),
            total_completion_tokens=usage.get("output_tokens"),
            total_cached_tokens=usage.get("cache_read_input_tokens"),
        ),
    )
    document = trajectory.model_dump(mode="json", exclude_none=True)
    validator = TrajectoryValidator()
    if not validator.validate(document):
        raise ValueError("Invalid Talos ATIF: " + ", ".join(validator.get_errors()))
    (logs / "trajectory.json").write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n")
