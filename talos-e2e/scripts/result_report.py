"""Persist runner outcomes and render a separate campaign report."""
import argparse
from collections import Counter, defaultdict
from decimal import Decimal
import fcntl
from html import escape
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time
from urllib.parse import quote

from report_failures import redact
from result_usage import load_prices, summarize_usage


def write_text(path, content):
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                     delete=False) as stream:
        temporary = Path(stream.name)
        try:
            stream.write(content)
            stream.close()
            temporary.replace(path)
        finally:
            temporary.unlink(missing_ok=True)


def write_json(path, data):
    write_text(path, json.dumps(data, ensure_ascii=False, indent=2) + "\n")


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def start(root, run_id, rerun_of, note, price_path, cases):
    baseline = None
    if rerun_of:
        if not re.fullmatch(r"\d{8}_\d{6}-\d+", rerun_of):
            raise ValueError("Expected the initial run ID in --rerun-of")
        baseline = read_json(root / "runs" / rerun_of / "run.json")
        if baseline["campaign"] != rerun_of or "exit_code" not in baseline:
            raise ValueError("--rerun-of must name a finished initial run")
        if not set(cases) <= set(baseline["cases"]):
            raise ValueError("The selected case is outside the initial run")
    if "\r" in note or "\n" in note:
        raise ValueError("--note must be a single line")
    note = redact(note, os.environ)
    prices = load_prices(price_path) if price_path else (baseline or {}).get("prices", {})
    revision = None
    if shutil.which("git"):
        commit = subprocess.run(["git", "-C", str(root.parent), "rev-parse", "HEAD"],
                                capture_output=True, text=True)
        revision = commit.stdout.strip() if commit.returncode == 0 else None
    directory = root / "runs" / run_id
    directory.mkdir(parents=True)
    write_json(directory / "run.json", {
        "id": run_id, "campaign": rerun_of or run_id, "cases": cases, "note": note,
        "prices": prices, "revision": revision,
        "started_ns": time.time_ns(),
    })


def attempts(root, runs):
    for run in runs:
        for case in run["cases"]:
            directory = root / "runs" / run["id"] / "cases" / case
            result_file = directory / "result.json"
            if result_file.is_file():
                result = read_json(result_file)
            elif (directory / "scenario.log").exists():
                result = {"status": "INCOMPLETE", "seconds": None, "usage": []}
            else:
                continue
            yield case, {**result, "run": run, "directory": directory,
                         "model_usage": summarize_usage(result.get("usage", []), run["prices"])}


def cell(value):
    return str(value).replace("\n", " ").replace("|", "&#124;").replace("\r", "")


def row(values):
    return "| " + " | ".join(cell(value) for value in values) + " |"


def table(headings):
    return [row(headings), row(["---"] * len(headings))]


def cost(models):
    amounts = defaultdict(lambda: 0)
    missing = 0
    for model in models:
        if model["cost"] is not None:
            amounts[model["currency"]] += model["cost"]
        missing += model["unpriced"]
    if not amounts:
        return f"N/A（{missing} 条无法估价）" if missing else "N/A"
    subtotal = " + ".join((f"<0.000001 {currency}" if 0 < value < Decimal("0.000001") else f"{value:.6f} {currency}")
                          for currency, value in sorted(amounts.items()))
    return subtotal + (f"（另有 {missing} 条无法估价）" if missing else "")


def observed(history):
    return [model for attempt in history for model in attempt["model_usage"]]


def duration(history):
    values = [a["seconds"] for a in history if a["seconds"] is not None]
    if not values:
        return "N/A"
    return f"{sum(values):.2f}" + ("（部分）" if len(values) != len(history) else "")


def stats(histories, initial):
    outcomes = Counter((history[0 if initial else -1]["status"] if history and
                        (not initial or history[0]["run"]["id"] == history[0]["run"]["campaign"])
                        else "UNRUN") for history in histories)
    passed, failed = outcomes["PASS"], outcomes["FAIL"]
    decided = passed + failed
    rates = f"{passed / decided:.1%} / {failed / decided:.1%}" if decided else "N/A"
    return f"{passed} / {failed} / {len(histories) - decided}", rates


def recovered(history):
    return (len(history) > 1 and history[0]["run"]["id"] == history[0]["run"]["campaign"]
            and history[0]["status"] == "FAIL" and history[-1]["status"] == "PASS")


def group_row(name, histories):
    first, first_rates = stats(histories, True)
    final, final_rates = stats(histories, False)
    return row([name, len(histories), first, first_rates,
                sum(len(h) > 1 for h in histories), sum(max(len(h) - 1, 0) for h in histories),
                sum(recovered(h) for h in histories), final, final_rates,
                duration([a for h in histories for a in h]), cost([m for h in histories for m in observed(h)])])


def render(root, runs, current):
    baseline = runs[0]
    by_case = {case: [] for case in baseline["cases"]}
    for case, attempt in attempts(root, runs):
        by_case[case].append(attempt)
    groups = defaultdict(list)
    for case, history in by_case.items():
        groups[case.split("/", 1)[0]].append((case, history))
    histories = list(by_case.values())
    all_models = [m for h in histories for m in observed(h)]
    first, _ = stats(histories, True)
    final, _ = stats(histories, False)
    model_names = sorted({f'{m["provider"]}/{m["model"]}' for m in all_models})
    inputs = sum(m["input"] or 0 for m in all_models)
    outputs = sum(m["output"] or 0 for m in all_models)
    cache_denominator = sum(m["cache_input"] for m in all_models)
    cache_hits = sum(m["cache_read"] or 0 for m in all_models)
    cache_rate = f"{cache_hits / cache_denominator:.1%}" if cache_denominator else "N/A"
    priced = cost(all_models)
    rerun_cost = cost([m for history in histories for m in observed(history[1:])])
    without_usage = sum(not attempt["model_usage"] for history in histories for attempt in history)
    snapshot_links = []
    for run in runs:
        if run["prices"]:
            snapshot = root / "runs" / run["id"] / "run.json"
            snapshot_links.append(f"[{run['id']}]({quote(snapshot.relative_to(root).as_posix())})")
    lines = [
        "# Talos E2E 执行结果", "",
        f"- 首轮 `{baseline['id']}`，本次 `{current['id']}`，退出码 {current['exit_code']}。",
        f"- 本次结束阶段 `{current['stage']}`。[预检日志](preflight_{current['id']}.log)。",
        f"- {len(groups)} 组，{len(histories)} 个 case，执行 {sum(map(len, histories))} 次。"
        f"首次成功 / 失败 / 其他为 {first}，最终为 {final}，补跑恢复 {sum(map(recovered, histories))} 个。",
        f"- 已观测模型 {'、'.join(model_names) or 'N/A'}，"
        f"输入 {inputs if any(m['input'] is not None for m in all_models) else 'N/A'}，"
        f"输出 {outputs if any(m['output'] is not None for m in all_models) else 'N/A'} tokens，"
        f"缓存命中 {cache_rate}（覆盖 {cache_denominator}/{inputs} 个已观测输入 tokens），"
        f"已观测估算费用 {priced}，其中补跑 {rerun_cost}。{without_usage} 次执行没有可归属的用量记录。", "",
        f"- 费率快照：{'、'.join(snapshot_links) or '未提供'}。", "",
        "费用按每次 run 的费率快照估算。汇总耗时和费用包含补跑。"
        "缓存命中率按有缓存读数据的输入 tokens 加权。Profile 是既有用量快照，"
        "无法覆盖所有失败、取消、未落盘或远端调用，金额只代表可估价的部分。", "",
        "## 分组汇总", "",
        *table(["Group", "Case 数", "首次成功 / 失败 / 其他", "首次通过率 / 失败率",
                "补跑 Case 数", "补跑次数", "恢复数", "最终成功 / 失败 / 其他",
                "最终通过率 / 失败率", "累计耗时（秒）", "观测费用"]),
    ]
    for group, cases in sorted(groups.items()):
        lines.append(group_row(group, [history for _, history in cases]))
    lines.append(group_row("合计", histories))
    for group, cases in sorted(groups.items()):
        lines += ["", f"## {group}", "",
                  *table(["Case", "首次", "最终", "执行次数", "补跑次数", "首次通过",
                          "累计耗时（秒）", "观测费用"])]
        for case, history in cases:
            first_pass = next((n for n, a in enumerate(history, 1) if a["status"] == "PASS"), None)
            unrun = "预检阻塞" if baseline["stage"] == "preflight" else "未执行"
            first_state = history[0]["status"] if history and history[0]["run"]["id"] == baseline["id"] else unrun
            lines.append(row([Path(case).name, first_state, history[-1]["status"] if history else unrun,
                              len(history), max(len(history) - 1, 0),
                              f"第 {first_pass} 次" if first_pass else "未通过",
                              duration(history), cost(observed(history))]))
        lines += ["", *table(["Case", "执行", "结果", "耗时（秒）", "观测费用", "用量来源", "代码版本", "备注", "证据"])]
        for case, history in cases:
            for n, attempt in enumerate(history, 1):
                run = attempt["run"]
                note = run["note"] or ("待定位，见日志" if attempt["status"] == "FAIL" else "—")
                log = attempt["directory"] / "scenario.log"
                lines.append(row([Path(case).name, f"第 {n} 次" + (f"（补跑 {n - 1}）" if n > 1 else ""),
                                  attempt["status"], duration([attempt]), cost(attempt["model_usage"]),
                                  attempt.get("coverage", "N/A"),
                                  (run["revision"] or "N/A")[:12], escape(note, quote=False),
                                  f"[日志]({quote(log.relative_to(root).as_posix())})"]))
        detail = [(Path(case).name, n, m) for case, history in cases
                  for n, attempt in enumerate(history, 1) for m in attempt["model_usage"]]
        if detail:
            lines += ["", *table(["Case", "执行", "配置模型", "输入", "输出", "总 Tokens", "缓存读", "缓存写",
                                  "缓存命中", "用量 / 缓存读 / 已定价（条）", "观测费用"])]
            for case, n, model in detail:
                rate = f"{model['cache_read'] / model['cache_input']:.1%}" if model["cache_input"] else "N/A"
                counts = [model[k] if model[k] is not None else "N/A"
                          for k in ("input", "output", "total", "cache_read", "cache_write")]
                coverage = " / ".join(f"{model[k]}/{model['rows']}" for k in ("metered", "cache_rows", "priced"))
                lines.append(row([case, n, f'{model["provider"]}/{model["model"]}', *counts, rate, coverage, cost([model])]))
    return "\n".join(lines) + "\n"


def finish(root, run_id, status, stage):
    directory = root / "runs" / run_id
    current = read_json(directory / "run.json")
    current.update(exit_code=int(status), stage=stage)
    campaign = current["campaign"]
    report = root / f"e2e_results_{campaign}.md"
    with (root / "runs" / campaign / ".report.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        write_json(directory / "run.json", current)
        runs = [run for path in (root / "runs").glob("*/run.json")
                if (run := read_json(path))["campaign"] == campaign and "exit_code" in run]
        runs.sort(key=lambda run: (run["id"] != campaign, run["started_ns"], run["id"]))
        write_text(report, render(root, runs, current))
    print(f"E2E results: {report} (run {run_id})")


def publish_case(root, run_id, case):
    """Publish the traditional case path and return its displayed duration."""
    source = root / "runs" / run_id / "cases" / case
    latest = root / case
    latest.parent.mkdir(parents=True, exist_ok=True)
    if not latest.parent.resolve().is_relative_to(root.resolve()):
        raise ValueError("Case report directory escapes the report root")
    with (root / "runs" / ".case-links.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if latest.is_dir() and not latest.is_symlink():
            legacy = root / "runs" / run_id / "legacy" / case
            legacy.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(latest), legacy)
        temporary = latest.with_name(latest.name + "." + run_id + ".tmp")
        temporary.symlink_to(os.path.relpath(source, latest.parent), target_is_directory=True)
        try:
            temporary.replace(latest)
        finally:
            temporary.unlink(missing_ok=True)
    receipt = source / "result.json"
    return duration([read_json(receipt)]) if receipt.is_file() else "N/A"


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("run_id")
    parser.add_argument("action", choices=("start", "finish", "publish"))
    parser.add_argument("args", nargs=argparse.REMAINDER)
    options = parser.parse_args()
    try:
        if options.action == "start":
            rerun_of, note, prices, *cases = options.args
            start(options.root, options.run_id, rerun_of, note, prices,
                  [case.removesuffix(".sh") for case in cases])
        elif options.action == "finish":
            finish(options.root, options.run_id, *options.args)
        else:
            print(publish_case(options.root, options.run_id, *options.args))
    except (OSError, ValueError) as error:
        parser.exit(1, f"E2E result report: {error}\n")
