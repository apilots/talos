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


def capture_events(database, destination, *, final=False):
    with sqlite3.connect(f"{database.as_uri()}?mode=ro", uri=True, timeout=1) as db:
        if not db.execute("SELECT 1 FROM sqlite_schema WHERE name = 'agent_events'").fetchone():
            return
        db.execute("BEGIN")
        events = db.execute("""
            WITH events AS (
                SELECT s.agent_path, e.turn_id, e.sequence, e.event_seq, e.occurred_at, e.payload_json,
                       CASE WHEN e.sequence IS NULL AND json_extract(e.payload_json, '$.level') = 'warning'
                            THEN 'warning' ELSE json_extract(e.payload_json, '$.type') END AS event_type,
                       json_extract(e.payload_json, '$.data.block.content.type') AS block_type
                FROM agent_events e JOIN sessions s ON e.session_id = s.session_key
            )
            SELECT agent_path, turn_id, sequence, occurred_at, event_type, block_type,
                   json_extract(payload_json, '$.data.block.content.data.name'),
                   json_extract(payload_json, '$.data.block.content.data.callId'),
                   json_extract(payload_json, '$.data.block.content.data.isError'),
                   CASE WHEN event_type = 'warning' THEN json_extract(payload_json, '$.content.data.code')
                        ELSE json_extract(payload_json, '$.data.error.code') END, event_seq,
                   json_extract(payload_json, '$.data.block.content.data.metrics.executionDurationMs'),
                   json_extract(payload_json, '$.data.block.content.data.metrics.queueDurationMs')
            FROM events
            WHERE event_type IN ('turnStarted', 'messageCompleted', 'turnCompleted', 'turnFailed', 'warning', 'waitingForInput')
               OR (event_type = 'messageBlockCompleted' AND block_type IN ('toolCall', 'toolResult'))
            ORDER BY event_seq
        """).fetchall()
        active = [
            {"path": path, "turnId": turn, "state": state, "lastEventSeq": last_event}
            for path, turn, state, last_event in db.execute("""
                SELECT s.agent_path, a.turn_id, a.state, a.last_event_seq
                FROM agent_active_turns a JOIN sessions s ON a.session_id = s.session_key
                ORDER BY a.last_event_seq
            """)
        ]
        observed_metrics = [
            {"path": path, "turnId": turn, "eventSeq": sequence, "modelRounds": rounds,
             "usage": json.loads(usage) if usage is not None else None}
            for path, turn, sequence, rounds, usage in db.execute("""
                WITH latest AS (
                    SELECT MAX(event_seq) AS event_seq
                    FROM agent_events
                    WHERE json_extract(payload_json, '$.type') = 'metricsUpdated'
                    GROUP BY session_id, turn_id
                )
                SELECT s.agent_path, e.turn_id, e.event_seq,
                       json_extract(e.payload_json, '$.data.modelRounds'),
                       json_extract(e.payload_json, '$.data.usage')
                FROM latest JOIN agent_events e ON e.event_seq = latest.event_seq
                JOIN sessions s ON s.session_key = e.session_id
                ORDER BY e.event_seq
            """)
        ]
        child_usage = []
        if final:
            child_usage = [
                {"path": path, "turnId": turn, "observedCalls": calls, "callsWithReportedCacheRead": measured,
                 "coveredInputTokens": inputs, "observedCacheReadTokens": reads,
                 "reportedInputTokens": total_inputs, "reportedCacheReadTokens": total_reads,
                 "observedRequests": requests, "callsWithCompleteUsage": complete,
                 "observedUsage": json.loads(observed), "reportedUsage": json.loads(reported)}
                for (path, turn, calls, measured, inputs, reads, total_inputs, total_reads,
                     requests, complete, observed, reported) in db.execute("""
                    WITH calls AS (
                        SELECT s.agent_path, e.session_id, e.turn_id, e.event_seq, r.key AS request_index,
                               json_extract(c.value, '$.metrics.usage.input_tokens') AS inputs,
                               json_extract(c.value, '$.metrics.usage.output_tokens') AS outputs,
                               json_extract(c.value, '$.metrics.usage.cache_read_input_tokens') AS cached,
                               json_extract(c.value, '$.metrics.usage.cache_creation_input_tokens') AS written
                        FROM agent_events e JOIN sessions s ON e.session_id = s.session_key,
                             json_each(e.execution_capture_json, '$.requests') AS r,
                             json_each(r.value, '$.request.callRecords') AS c
                        WHERE s.agent_path IS NOT NULL
                    )
                    SELECT agent_path, calls.turn_id, COUNT(*), COUNT(cached),
                           SUM(CASE WHEN cached IS NOT NULL THEN inputs END), SUM(cached),
                           p.input_tokens, p.cache_read_input_tokens,
                           COUNT(DISTINCT event_seq || ':' || request_index),
                           COUNT(CASE WHEN outputs >= 0 AND cached >= 0 AND written >= 0
                                           AND cached + written <= inputs THEN 1 END),
                           json_object('input_tokens', SUM(inputs), 'output_tokens', SUM(outputs),
                                       'cache_read_input_tokens', SUM(cached),
                                       'cache_creation_input_tokens', SUM(written)),
                           json_object('input_tokens', p.input_tokens, 'output_tokens', p.output_tokens,
                                       'cache_read_input_tokens', p.cache_read_input_tokens,
                                       'cache_creation_input_tokens', p.cache_creation_input_tokens)
                    FROM calls LEFT JOIN profile_turn_usage p ON p.session_id = calls.session_id
                        AND p.turn_id = calls.turn_id AND p.usage_kind = 'turn' AND p.finalized = 1
                        AND p.usage_available = 1
                    GROUP BY calls.session_id, calls.turn_id
                    ORDER BY agent_path, calls.turn_id
                """)
            ]
    snapshot = {
        "fields": ["path", "turnId", "sequence", "occurredAt", "event", "block", "tool",
                   "callId", "toolError", "errorCode", "eventSeq", "toolExecutionMs", "toolQueueMs"],
        "events": events,
        "activeTurns": active,
        "observedTurnMetrics": observed_metrics,
    }
    if final:
        snapshot["childUsage"] = child_usage
    temporary = destination.with_suffix(".tmp")
    temporary.write_text(json.dumps(snapshot, separators=(",", ":")) + "\n")
    temporary.replace(destination)


def record_events(database, destination, evidence, *, final=False):
    if database.is_file():
        try:
            capture_events(database, destination, final=final)
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
                record_events(database, events, evidence, final=True)
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
                record_events(database, events, evidence, final=True)
                return 124
