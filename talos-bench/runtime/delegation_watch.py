"""Record collaboration progress during an opt-in Talos Bench trial."""

from collections import deque
import json
from pathlib import Path
import sqlite3
import subprocess
import time


SAMPLE_SECONDS = 30
NO_PROGRESS_SECONDS = 12 * 60
GROWTH_SECONDS = 3 * 60


def worktree_signature(workspace, paths):
    if not paths and not (workspace / ".git").exists():
        return None
    if paths:
        files = []
        for path in paths:
            target = workspace / path
            files.append(target)
            if target.is_dir():
                files.extend(sorted(target.rglob("*")))
    else:
        result = subprocess.run(["git", "--no-optional-locks", "-C", workspace, "status", "--porcelain", "--no-renames",
                                 "--untracked-files=all", "-z"], capture_output=True, check=True)
        records = result.stdout.decode(errors="surrogateescape").split("\0")
        files = [workspace / record[3:] for record in sorted(filter(None, records))]
    stamps = []
    for path in files:
        try:
            stat = path.stat()
        except FileNotFoundError:
            stamps.append((path, None))
        else:
            stamps.append((path, (stat.st_size, stat.st_mtime_ns)))
    return stamps


def collaboration_snapshot(database):
    try:
        with sqlite3.connect(f"{database.as_uri()}?mode=ro", uri=True, timeout=1) as db:
            if not db.execute("SELECT 1 FROM sqlite_schema WHERE name = 'agent_events'").fetchone():
                return None
            sessions = [row[0] for row in db.execute("SELECT agent_path FROM sessions WHERE agent_path IS NOT NULL")]
            turns = dict(db.execute("""
                SELECT json_extract(e.payload_json, '$.type'), COUNT(*)
                FROM agent_events e JOIN sessions s ON e.session_id = s.session_key
                WHERE s.agent_path IS NOT NULL
                  AND json_extract(e.payload_json, '$.type') IN ('turnCompleted', 'turnFailed')
                GROUP BY 1
            """))
    except sqlite3.OperationalError as error:
        if error.sqlite_errorcode in (sqlite3.SQLITE_CANTOPEN, sqlite3.SQLITE_BUSY):
            return None  # Talos creates and migrates the database after process startup.
        raise
    return {"childSessions": len(sessions), "maxDepth": max((path.count("/") - 1 for path in sessions), default=0),
            "childTurns": turns}


def should_stop(sample):
    return (sample["secondsWithoutProgress"] >= NO_PROGRESS_SECONDS
            and sample["childSessions"] >= 12 and sample["maxDepth"] >= 4 and sample["recentChildGrowth"] >= 3)


def capture_events(database, destination):
    with sqlite3.connect(f"{database.as_uri()}?mode=ro", uri=True, timeout=1) as db:
        if not db.execute("SELECT 1 FROM sqlite_schema WHERE name = 'agent_events'").fetchone():
            return
        events = db.execute("""
            WITH events AS (
                SELECT s.agent_path, e.turn_id, e.sequence, e.event_seq, e.occurred_at, e.payload_json,
                       json_extract(e.payload_json, '$.type') AS event_type,
                       json_extract(e.payload_json, '$.data.block.content.type') AS block_type
                FROM agent_events e JOIN sessions s ON e.session_id = s.session_key
            )
            SELECT agent_path, turn_id, sequence, occurred_at, event_type, block_type,
                   json_extract(payload_json, '$.data.block.content.data.name'),
                   json_extract(payload_json, '$.data.block.content.data.callId'),
                   json_extract(payload_json, '$.data.block.content.data.isError'),
                   json_extract(payload_json, '$.data.error.code'), event_seq,
                   json_extract(payload_json, '$.data.block.content.data.metrics.executionDurationMs'),
                   json_extract(payload_json, '$.data.block.content.data.metrics.queueDurationMs')
            FROM events
            WHERE event_type IN ('turnStarted', 'turnCompleted', 'turnFailed', 'waitingForInput')
               OR (event_type = 'messageBlockCompleted' AND block_type IN ('toolCall', 'toolResult'))
            ORDER BY event_seq
        """).fetchall()
    temporary = destination.with_suffix(".tmp")
    temporary.write_text(json.dumps({
        "fields": ["path", "turnId", "sequence", "occurredAt", "event", "block", "tool",
                   "callId", "toolError", "errorCode", "eventSeq", "toolExecutionMs", "toolQueueMs"],
        "events": events,
    }, separators=(",", ":")) + "\n")
    temporary.replace(destination)


def record_events(database, destination, evidence):
    if database.is_file():
        try:
            capture_events(database, destination)
        except sqlite3.OperationalError as error:
            evidence.write(json.dumps({"captureError": error.sqlite_errorname}) + "\n")
        except OSError as error:
            evidence.write(json.dumps({"captureError": error.errno}) + "\n")


def wait(process, home, logs, workspace, paths, signature):
    database = Path(home) / "data/talos.db"
    last_progress = started = time.monotonic()
    history = deque()
    completed = 0
    events = logs / "talos-delegation-events.json"
    with (logs / "talos-delegation.jsonl").open("w", buffering=1) as evidence:
        while True:
            try:
                exit_code = process.wait(timeout=SAMPLE_SECONDS)
            except subprocess.TimeoutExpired:
                pass
            else:
                record_events(database, events, evidence)
                return exit_code
            now = time.monotonic()
            snapshot = collaboration_snapshot(database)
            current = worktree_signature(workspace, paths)
            completed_now = snapshot["childTurns"].get("turnCompleted", 0) if snapshot is not None else completed
            if current != signature or completed_now > completed:
                last_progress = now
                signature = current
            if snapshot is None:
                continue
            completed = completed_now
            history.append((now, snapshot["childSessions"]))
            while len(history) > 1 and history[1][0] <= now - GROWTH_SECONDS:
                history.popleft()
            growth = snapshot["childSessions"] - history[0][1]
            sample = {**snapshot, "elapsedSeconds": round(now - started, 1),
                      "secondsWithoutProgress": round(now - last_progress, 1), "recentChildGrowth": growth}
            stop = signature is not None and should_stop(sample) and process.poll() is None
            if stop:
                sample["stopReason"] = "recursiveDelegationWithoutProgress"
            evidence.write(json.dumps(sample) + "\n")
            record_events(database, events, evidence)
            if stop:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                return 124
