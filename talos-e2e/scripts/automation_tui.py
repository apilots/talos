"""Read canonical facts produced by the real terminal's two automation entries."""
import json
from pathlib import Path
import sqlite3
import sys
import time


def definition(connection, mode, marker):
    table, field = ("session_goals", "objective") if mode == "goal" else ("session_loops", "prompt")
    rows = connection.execute(f"SELECT * FROM {table} WHERE {field} LIKE ?", (f"AUTO_CASE={marker} %",)).fetchall()
    assert len(rows) == 1, f"Expected one exact {mode} created by the slash input, got {len(rows)}"
    return dict(rows[0])


def terminal(connection, mode, marker):
    task = definition(connection, mode, marker)
    if task["status"] != ("complete" if mode == "goal" else "stopped"):
        return False
    events = connection.execute("SELECT payload_json FROM agent_events WHERE turn_id=? AND sequence IS NOT NULL", (task["latest_turn_id"],))
    return any(json.loads(row[0])["type"] == "turnCompleted" for row in events)


def verify(connection, goal_marker, loop_marker):
    tasks = {mode: definition(connection, mode, marker)
             for mode, marker in (("goal", goal_marker), ("loop", loop_marker))}
    session_id = tasks["goal"]["session_id"]
    assert tasks["loop"]["session_id"] == session_id, "Both slash entries must stay in the original Session"
    assert tasks["goal"]["status"] == "complete", tasks["goal"]
    assert tasks["loop"]["stop_reason"] == "modelStopped", tasks["loop"]
    turns = {}
    product_errors = [json.loads(row[0]) for row in connection.execute(
        "SELECT payload_json FROM agent_events WHERE session_id=? AND sequence IS NULL AND json_extract(payload_json,'$.content.type')='error' ORDER BY event_seq",
        (session_id,))]
    assert not product_errors, {"unexpectedProductErrors": product_errors}
    events = connection.execute("SELECT * FROM agent_events WHERE session_id=? AND sequence IS NOT NULL ORDER BY event_seq", (session_id,))
    for row in events:
        event = json.loads(row["payload_json"])
        turn = turns.setdefault(row["turn_id"], {"turnId": row["turn_id"]})
        if event["type"] == "turnStarted":
            turn["startedAt"] = row["occurred_at"]
        elif event["type"] in ("turnCompleted", "turnFailed", "turnCancelled"):
            turn["terminal"] = event["type"]
        elif event["type"] == "messageCompleted" and event["data"]["role"] == "user":
            turn["input"] = [block["content"]["data"] for block in event["data"]["blocks"]
                             if block["content"]["type"] == "userContent"]
    assert len(turns) == 4 and all(turn.get("terminal") == "turnCompleted" for turn in turns.values()), turns
    for mode, task in tasks.items():
        work = turns[task["latest_turn_id"]]
        task["firstWorkDelaySeconds"] = work["startedAt"] - task["created_at"]
        assert task["firstWorkDelaySeconds"] >= 10, task
        selected = [turn for turn in turns.values() if any(item.get("type") == "skillSelection"
                    and item.get("name") == mode for item in turn.get("input", []))]
        assert len(selected) == 2, f"Original selected and automatic {mode} inputs must carry SkillSelection"
        activations = [turn for turn in selected if turn["turnId"] != task["latest_turn_id"]]
        assert len(activations) == 1 and any(item.get("type") == "text"
               and "controlled automation work" in item["text"] for item in activations[0]["input"]), selected
    return {"result": "PASS", "sessionId": session_id, "tasks": tasks, "turns": list(turns.values())}


if __name__ == "__main__":
    action, database = sys.argv[1:3]
    with sqlite3.connect(f"file:{Path(database)}?mode=ro", uri=True) as connection:
        connection.row_factory = sqlite3.Row
        if action == "await-terminal":
            deadline = time.monotonic() + 30
            while not terminal(connection, sys.argv[3], sys.argv[4]):
                assert time.monotonic() < deadline, "Automatic Turn did not settle normally"
                time.sleep(0.1)
        elif action == "verify":
            result = verify(connection, sys.argv[3], sys.argv[4])
            Path(sys.argv[5]).write_text(json.dumps(result, indent=2))
        else:
            raise ValueError(f"Unknown action: {action}")
