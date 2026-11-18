"""Read normalized usage from E2E-owned Talos homes and estimate observed cost."""
from collections import defaultdict
from contextlib import closing
from decimal import Decimal, InvalidOperation
import json
import os
from pathlib import Path
import re
import sqlite3

RATES = ("input", "output", "cache_read", "cache_write")
COUNTERS = ("input", "output", "total", "cache_read", "cache_write")


def load_prices(path):
    document = json.loads(Path(path).read_text(), parse_float=Decimal)
    if not isinstance(document, dict):
        raise ValueError("Price snapshot must be an object keyed by provider/model")
    snapshot = {}
    for key, entry in document.items():
        if "/" not in key or not isinstance(entry, dict):
            raise ValueError("Expected prices keyed by provider/model")
        currency, source, as_of = (entry.get(k) for k in ("currency", "source", "as_of"))
        if (not isinstance(currency, str) or not re.fullmatch(r"[A-Z]{3}", currency)
                or not isinstance(source, str) or not source.strip()
                or not isinstance(as_of, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", as_of)):
            raise ValueError(f"Price for {key} requires currency, source and as_of (YYYY-MM-DD)")
        try:
            rates = {name: Decimal(str(entry[name])) for name in RATES}
        except (KeyError, InvalidOperation) as error:
            raise ValueError(f"Price for {key} requires four per-million rates") from error
        if any(not rate.is_finite() or rate < 0 for rate in rates.values()):
            raise ValueError(f"Invalid rates for {key}")
        free = entry.get("free") is True
        if free != all(rate == 0 for rate in rates.values()):
            raise ValueError(f"Set free=true only when all rates for {key} are explicitly zero")
        snapshot[key] = {"currency": currency, "source": source, "as_of": as_of, "free": free,
                         **{name: str(rate) for name, rate in rates.items()}}
    return snapshot


def owned_databases(directory):
    for folder, dirs, files in os.walk(directory, followlinks=False):
        dirs[:] = [d for d in dirs if d not in {".git", "node_modules", "target"}]
        parent = Path(folder)
        if parent.name == "data" and "talos.db" in files:
            database = parent / "talos.db"
            if not database.is_symlink() and database.is_file():
                yield database


def collect_usage(*directories):
    samples, conflicts = {}, set()
    databases = unreadable = 0
    for directory in directories:
        for database in owned_databases(directory):
            try:
                with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=1)) as db:
                    rows = db.execute("""
                        SELECT user_id, session_id, turn_id, last_request_id, usage_kind,
                               model_provider, model_id, usage_available, finalized,
                               input_tokens, output_tokens, total_tokens,
                               cache_read_input_tokens, cache_creation_input_tokens
                        FROM profile_turn_usage
                    """).fetchall()
            except sqlite3.Error:
                unreadable += 1
                continue
            databases += 1
            for user, session, turn, request, kind, provider, model, available, finalized, *counts in rows:
                key = (user, kind, session, turn) if kind == "turn" else (user, kind, request)
                sample = {"provider": provider, "model": model, "kind": kind, "finalized": bool(finalized),
                          **dict(zip(COUNTERS, counts if available else (None,) * len(COUNTERS)))}
                previous = samples.get(key)
                if previous is None or (sample["finalized"] and not previous["finalized"]):
                    samples[key] = sample
                    conflicts.discard(key)
                elif sample["finalized"] == previous["finalized"] and sample != previous:
                    conflicts.add(key)
    observations = [sample for key, sample in samples.items() if key not in conflicts]
    if not databases:
        coverage = "N/A（无可读快照）"
    else:
        coverage = f"Profile 部分观测：{databases} 库，{len(observations)} 条"
    if unreadable or conflicts:
        coverage += f"，{unreadable} 库无法读取，{len(conflicts)} 条冲突"
    if unfinished := sum(not sample["finalized"] for sample in observations):
        coverage += f"，{unfinished} 条未结算"
    return observations, coverage


def estimate(sample, prices):
    if prices is None or sample["input"] is None or sample["output"] is None:
        return None
    inputs, outputs = sample["input"], sample["output"]
    rates = {name: Decimal(prices[name]) for name in RATES}
    reads, writes = sample["cache_read"], sample["cache_write"]
    if (reads is None and rates["cache_read"] != rates["input"]
            or writes is None and rates["cache_write"] != rates["input"]):
        return None
    reads, writes = reads or 0, writes or 0
    if min(inputs, outputs, reads, writes) < 0 or reads + writes > inputs:
        return None
    return ((inputs - reads - writes) * rates["input"] + outputs * rates["output"]
            + reads * rates["cache_read"] + writes * rates["cache_write"]) / 1_000_000


def summarize_usage(samples, prices):
    by_model = defaultdict(list)
    for sample in samples:
        by_model[sample["provider"], sample["model"]].append(sample)
    results = []
    for (provider, model), records in sorted(by_model.items()):
        price = prices.get(f"{provider}/{model}")
        measured = [r for r in records if r["input"] is not None and r["output"] is not None]
        cached = [r for r in measured if r["cache_read"] is not None and 0 <= r["cache_read"] <= r["input"]]
        priced = [value for record in measured if (value := estimate(record, price)) is not None]
        totals = {}
        for key in COUNTERS:
            source = cached if key == "cache_read" else measured
            values = [r[key] for r in source if r[key] is not None]
            totals[key] = sum(values) if values else None
        results.append({"provider": provider, "model": model, "rows": len(records),
                        "metered": len(measured), "cache_rows": len(cached), "priced": len(priced),
                        "unpriced": len(records) - len(priced), "cache_input": sum(r["input"] for r in cached),
                        "cost": sum(priced, Decimal(0)) if priced else None,
                        "currency": price["currency"] if price else None, **totals})
    return results
