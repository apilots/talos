"""Render one registered suite's local coverage and attempt evidence as HTML."""

import argparse
from collections import defaultdict
from html import escape
import math
import os
from pathlib import Path
from urllib.parse import quote

from summarize import read_dataset, read_json


ARTIFACT_LABELS = {
    "harborResult": "原始评分", "trialLog": "执行日志", "trajectory": "Talos 轨迹",
    "atif": "ATIF", "transcript": "消息记录", "usage": "Token 用量",
    "stderr": "Agent 日志", "submissionManifest": "提交物清单", "architecture": "运行架构",
    "delegationWatch": "委派时序", "delegationEvents": "子会话与工具事件",
}


def link(target, label, base, evidence_root):
    path = target.resolve()
    if not path.is_file() or not path.is_relative_to(evidence_root):
        return ""
    url = quote(os.path.relpath(path, base))
    return f'<a href="{escape(url)}">{escape(label)}</a>'


def valid_score(trial, run, require_atif):
    score = trial.get("score")
    files = trial.get("artifacts", {})
    required = ("trajectory", "atif") if require_atif else ("trajectory",)
    return (type(score) in (int, float) and math.isfinite(score) and 0 <= score <= 1
            and trial.get("cliExitCode") == 0 and not trial.get("exception")
            and trial.get("modelCallsObserved") and not trial.get("trajectoryError")
            and all(key in files and (run / files[key]).is_file() for key in required)
            and all(item["path"] and (run / item["path"]).exists()
                    for item in trial.get("submissions", [])))


def attempt_reason(trial, run):
    if trial.get("stopReason") == "recursiveDelegationWithoutProgress":
        return "Bench 中止：子 Agent 持续递归委派，未观察到交付进展。"
    if trial.get("exception"):
        return trial["exception"]
    if trial.get("score") is None:
        return "没有有效评分，见原始运行记录"
    if trial["score"] == 1:
        return ""
    verifier = trial.get("artifacts", {}).get("verifier")
    if not verifier:
        return "评分低于满分，原因待复盘"
    directory = run / verifier
    tests = read_json(directory / "ctrf.json").get("results", {}).get("tests", [])
    failed = [item["name"] for item in tests if item["status"] in ("fail", "failed")]
    reward = read_json(directory / "reward-details.json").get("reward")
    if isinstance(reward, dict):
        failed.extend(item["name"] for item in reward.get("criteria", []) if item.get("value") == 0)
    return "未通过 " + "、".join(failed[:8]) if failed else "评分低于满分，原因待复盘"


def load_attempts(dataset, runs, registered, model, reasoning_effort=None):
    attempts = defaultdict(list)
    expected_dataset = f"{dataset['source']}@{dataset['digest']}" if "source" in dataset else None
    upstream_revision = (dataset.get("upstream") or {}).get("revision")
    for path in sorted(runs.glob("*/summary.json"), key=lambda path: path.stat().st_mtime):
        summary = read_json(path)
        if (summary.get("suite"), summary.get("suiteVersion"), summary.get("mode")) != (
            dataset["name"], dataset["version"], "real"
        ) or (expected_dataset and summary.get("datasetRef") != expected_dataset):
            continue
        for trial in summary["trials"]:
            name = (trial.get("task") or "").rsplit("/", 1)[-1]
            if name not in registered or trial.get("model") not in (model, None):
                continue
            if reasoning_effort is not None and trial.get("reasoningEffort") != reasoning_effort:
                continue
            entry = registered[name]
            expected_revision = entry.get("git_commit_id") or entry.get("revision") or upstream_revision
            provenance = read_json(path.parent / "provenance" / f"{name}.json")
            revision = trial.get("taskRevision") or provenance.get("datasetRevision")
            task_ref = provenance.get("upstreamTaskRef") or provenance.get("taskRef", "")
            digest = trial.get("taskDigest")
            if task_ref and provenance.get("localTaskDigest") == digest:
                digest = task_ref.partition("@")[2]
            if (expected_revision and revision != expected_revision) or (entry.get("digest") and digest != entry["digest"]):
                continue
            attempts[name].append((path, trial))
    return attempts


def render_attempt(attempt, target, evidence_root, notes, require_atif):
    summary, trial = attempt
    run = summary.parent
    files = trial.get("artifacts", {})
    links = [link(summary, "摘要", target, evidence_root)]
    links.append(link(run / "build.json", "构建记录", target, evidence_root))
    provenance = run / "provenance" / f"{trial['task'].rsplit('/', 1)[-1]}.json"
    links.append(link(provenance, "任务与镜像来源", target, evidence_root))
    links.extend(link(run / files[key], label, target, evidence_root)
                 for key, label in ARTIFACT_LABELS.items() if key in files)
    if "harborResult" in files:
        trial_dir = (run / files["harborResult"]).parent
        links.append(link(trial_dir / "artifacts/atlas-sandbox.log", "Sandbox 日志", target, evidence_root))
    if "verifier" in files:
        verifier = run / files["verifier"]
        links.extend(link(verifier / name, label, target, evidence_root)
                     for name, label in (("test-stdout.txt", "Verifier 日志"),
                                         ("ctrf.json", "测试结果"),
                                         ("reward-details.json", "评分细项"),
                                         ("swe-bench-report.json", "SWE-bench 评分"),
                                         ("swe-bench-test-output.txt", "SWE-bench 测试日志"),
                                         ("scored_talos.csv", "Atlas 评分"),
                                         ("judge-verdicts.json", "Judge 判定")))
    links.extend(link(run / item["path"], "提交物", target, evidence_root)
                 for item in trial.get("submissions", []) if item.get("path"))
    note = notes.get(f"{run.name}/{trial['task']}")
    reason = note or attempt_reason(trial, run)
    if trial.get("score") is not None and not valid_score(trial, run, require_atif):
        reason = "真实模型证据不完整。" + reason
    usage = trial.get("usage") or {}
    fields = (
        ("试次用时", trial.get("trialSeconds")), ("环境准备", trial.get("environmentSetupSeconds")),
        ("Agent 阶段", trial.get("agentSeconds")), ("Talos CLI", trial.get("cliSeconds")),
        ("评分", trial.get("verifierSeconds")),
        ("模型调用累计", trial.get("observedModelSeconds")), ("工具执行累计", trial.get("observedToolSeconds")),
    )
    durations = [f"{label} {value:.2f} 秒" for label, value in fields if value is not None]
    fields = (
        ("根 Turn 输入", usage.get("input_tokens")),
        ("输出", usage.get("output_tokens")), ("缓存读", usage.get("cache_read_input_tokens")),
        ("缓存写", usage.get("cache_creation_input_tokens")),
        ("模型调用", trial.get("modelCallsObserved")), ("工具调用", trial.get("toolCalls")),
        ("工具错误", trial.get("toolErrors")),
    )
    metrics = " · ".join([*durations, *(f"{label} {value:,}" for label, value in fields if value is not None)])
    if cache := trial.get("rootCacheRead"):
        state = cache["status"]
        if state == "verified":
            metrics += f" · 根 Turn 缓存读占比 {cache['ratio']:.2%}（已对账）"
        elif state == "partial":
            metrics += (f" · 根 Turn 缓存读覆盖 {cache['callsWithReportedCacheRead']}/{cache['observedCalls']} 次可见调用"
                        f"、{cache['coveredInputTokens']:,} 输入 tokens，无法完整对账")
        elif state == "inconsistent":
            metrics += " · 根 Turn 缓存读计数不一致"
        else:
            metrics += " · 根 Turn 缓存读占比不可用"
    if (cost := trial.get("estimatedCostUsd")) is not None:
        basis = "已对账" if trial.get("rootCostReconciled") else "按归档汇总"
        metrics += f" · 根 Turn 估算成本 ${cost:.4f}（{basis}）"
    if workload := trial.get("workloadUsage"):
        total = workload["descendantTurnsObserved"]
        usage = workload["observedDescendantUsage"] or {}
        if total:
            metrics += (f" · 子 Turn 用量已观测 {workload['descendantTurnsWithUsage']}/{total} 个，"
                        f"缓存对账 {workload['descendantTurnsReconciled']}/{total} 个")
            values = [
                f"{label} {usage[key]:,}" for label, key in (
                    ("子 Turn 输入", "input_tokens"), ("输出", "output_tokens"),
                    ("缓存读", "cache_read_input_tokens"), ("缓存写", "cache_creation_input_tokens"),
                ) if usage.get(key) is not None
            ]
            if values:
                metrics += " · " + " · ".join(values)
        if (ratio := workload["cacheReadRatio"]) is not None:
            metrics += f" · Agent Turn 合计缓存读占比 {ratio:.2%}（已对账）"
        if trial.get("rootCostReconciled") and (cost := workload["estimatedCostUsd"]) is not None:
            metrics += f" · Agent Turn 合计估算成本 ${cost:.4f}"
        elif (cost := workload["reconciledDescendantEstimatedCostUsd"]) is not None:
            basis = "逐调用对账" if "rootCostReconciled" in trial else "按归档汇总"
            metrics += (f" · {workload['pricedDescendantTurns']}/{total} 个子 Turn"
                        f"估算成本 ${cost:.4f}（{basis}）")
        if (cost := workload.get("descendantEstimateFromReportedTotalsUsd")) is not None:
            metrics += (f" · {workload['descendantTurnsEstimatedFromReportedTotals']}/{total} 个子 Turn"
                        f"估算成本 ${cost:.4f}（按历史汇总计数，缺少逐调用计费证据）")
    collaboration = "单 Agent 诊断" if trial.get("collaborationTools") is False else None
    label = " · ".join(str(x) for x in (
        run.name, trial.get("model"), trial.get("reasoningEffort"), trial.get("taskDigest"), collaboration,
    ) if x)
    raw = trial.get("verifierReward", trial.get("score"))
    if judge := read_json(provenance).get("judgeModel"):
        label += f" · Judge {judge}"
    if raw is not None:
        label += f" · Runner 原始分 {raw:g}"
    episode_path = next((item["path"] for item in trial.get("submissions", [])
                         if item["source"] == "/logs/artifacts/episode.json" and item.get("path")), None)
    if episode_path and (episode := read_json(run / episode_path)):
        if episode.get("done"):
            label += f" · MiniWoB 原环境奖励 {episode['env_reward']:g} · 无时间权重 {episode['raw_reward']:g}"
        else:
            label += " · MiniWoB 回合未结束"
        if episode.get("reason"):
            reason = " · ".join(filter(None, (reason, str(episode["reason"]))))
    return f"<li>{escape(label)}<br>{escape(metrics)}<br>{escape(reason)} {' '.join(filter(None, links))}</li>"


def render(output, registry, runs, model, notes, cohort, *, reasoning_effort=None):
    dataset = read_dataset(registry)
    titles = (dataset["name"], dataset["version"])
    require_atif = titles in (("terminal-bench", "4.1.0"), ("miniwob", "1.1"))
    pass_threshold = dataset.get("pass_threshold", 1.0)
    registered = {row["name"].rsplit("/", 1)[-1]: row for row in dataset["tasks"]}
    names = {name.rsplit("/", 1)[-1] for name in cohort} if cohort else registered.keys()
    if unknown := names - registered.keys():
        raise ValueError(f"Unregistered report tasks: {', '.join(sorted(unknown))}")
    attempts = load_attempts(dataset, runs, registered, model, reasoning_effort)
    output_root = runs.parent.resolve()
    selected = sorted(names)
    rows, scores = [], []
    started = 0
    for name in selected:
        entries = attempts[name]
        eligible = [trial["score"] for path, trial in entries
                    if trial.get("model") == model and trial.get("collaborationTools") is not False
                    and valid_score(trial, path.parent, require_atif)]
        score = eligible[-1] if eligible else None
        runner_logs = [link(output.parent / f"{name}.log", "Runner 日志", output.parent, output_root)]
        runner_logs += [link(path, path.name, output.parent, output_root)
                        for path in sorted(output.parent.glob(f"{name}.*.log"))]
        case_note = escape(notes.get(name, ""))
        attempted = bool(entries or (reasoning_effort is None and any(runner_logs)))
        started += attempted
        if score is not None:
            scores.append(score)
        if score is None:
            status = "暂无有效评分" if attempted else "未运行"
        elif score >= pass_threshold:
            status = "通过"
        else:
            status = "零分" if score == 0 else "低于通过线"
        history_rows = "".join(render_attempt(entry, output.parent, output_root, notes, require_atif)
                               for entry in reversed(entries))
        details = (f"<details><summary>{len(entries)} 次记录</summary><ul>{history_rows}</ul></details>"
                   if entries else "—")
        description = registered[name].get("description")
        title = f"{escape(name)}<br><small>{escape(description)}</small>" if description else escape(name)
        rows.append(f"<tr><th scope=\"row\">{title}</th><td>{status}</td>"
                    f"<td>{score if score is not None else '—'}</td>"
                    f"<td>{case_note} {details} {' '.join(filter(None, runner_logs))}</td></tr>")
    title = escape(" ".join(titles))
    profile = escape(f"{model} · 推理档位 {reasoning_effort or '不限'}")
    passed = sum(score >= pass_threshold for score in scores)
    average = f"{sum(scores) / len(scores):.4f}" if scores else "—"
    counters = [("代表题", len(names)), ("已开始", started), ("有效评分", len(scores)),
                ("通过", passed), ("零分", scores.count(0)), ("未出有效分", started - len(scores)),
                ("未运行", len(names) - started),
                ("平均分", average)]
    figures = "".join(f"<div><strong>{value}</strong><span>{label}</span></div>" for label, value in counters)
    content = f"""<!doctype html>
<html lang="zh-CN">
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} 代表题评估</title>
<style>
:root{{font:16px/1.5 system-ui;color:#192230;background:#f4f6f9}}
body{{max-width:1180px;margin:2rem auto;padding:0 1rem}}
h1{{line-height:1.2}} a{{color:#174e90;margin-right:.4em}} summary{{cursor:pointer}}
.figures{{display:grid;grid-template-columns:repeat(auto-fit,minmax(105px,1fr));gap:.75rem;margin:1.25rem 0}}
.figures div{{padding:.75rem;background:#fff;border:1px solid #d7dee9;border-radius:6px}}
strong,span{{display:block}} strong{{font-size:1.45rem}}
.table{{overflow-x:auto}} table{{width:100%;border-collapse:collapse;background:#fff}}
th,td{{text-align:left;vertical-align:top;padding:.65rem;border-bottom:1px solid #d7dee9}}
th,li{{overflow-wrap:anywhere}} thead{{background:#e7edf5}} ul{{padding-left:1.1rem}} li{{margin:.6rem 0}}
</style>
<main><h1>{title} 本地代表题评估</h1>
<p>{profile} · 通过线 {pass_threshold:g} · 满分 {scores.count(1)} 题。逐题采用最近一次产物齐全的有效评分。</p>
<section class="figures" aria-label="结果汇总">{figures}</section>
<div class="table"><table><thead><tr><th scope="col">题目</th><th scope="col">状态</th>
<th scope="col">得分</th><th scope="col">尝试与证据</th></tr></thead>
<tbody>{''.join(rows)}</tbody></table></div></main></html>
"""
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(".tmp")
    temporary.write_text(content)
    temporary.replace(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("registry", type=Path)
    parser.add_argument("--runs", type=Path, default=Path("output/talos-bench"))
    parser.add_argument("--model", required=True)
    parser.add_argument("--reasoning-effort", help="Limit the report to an exact recorded reasoning effort")
    parser.add_argument("--notes", type=Path)
    parser.add_argument("--task", action="append", default=[])
    args = parser.parse_args()
    render(args.output, args.registry, args.runs, args.model,
           read_json(args.notes) if args.notes else {}, args.task, reasoning_effort=args.reasoning_effort)


if __name__ == "__main__":
    main()
