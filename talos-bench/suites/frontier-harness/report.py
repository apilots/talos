"""Render the local FrontierHarness v1.1 full-run inventory and evidence."""

import argparse
from collections import Counter
from html import escape
import json
import math
import os
from pathlib import Path
from tempfile import NamedTemporaryFile
from urllib.parse import quote


DATASET_SIZE = 62
PREFLIGHT_LABELS = {
    "ready": "可原生运行",
    "needsNativeBuild": "需原生构建",
    "blocked": "环境受限",
    "unresolved": "待核对",
}
RESULT_LABELS = {
    "scored": "已评分",
    "modelFailure": "模型失败",
    "infraFailure": "基础设施故障",
    "blocked": "环境受限",
    "unresolved": "待核对",
    "missing": "缺少结果",
    "unrun": "未运行",
}
REASON_LABELS = {
    "AgentTimeoutError": "Agent 超时",
    "AgentSafetyRefusalError": "Agent 拒绝执行",
    "batchPreparationOrSummaryFailed": "任务准备或摘要生成失败",
    "incompleteArtifacts": "评分完成，归档缺少必需产物",
    "providerRequestTimeout": "模型服务请求超时",
    "providerRateLimit": "模型服务限流",
    "runnerDidNotScore": "Runner 未返回评分",
    "verifierDependencyBootstrap": "Verifier 依赖初始化失败",
    "verifierBrowserDriverUnavailable": "Verifier 无法启动 ChromeDriver",
    "verifierRewardBelowOne": "Verifier 给分低于 1",
}
ARTIFACT_LABELS = {
    "harborResult": "Harbor trial",
    "atif": "ATIF",
    "trialLog": "Trial 日志",
    "stderr": "Agent 日志",
    "verifier": "Verifier",
    "usage": "Token 用量",
    "submissionManifest": "提交清单",
}
TOKEN_KEYS = (
    "input_tokens", "output_tokens", "cache_read_input_tokens", "cache_creation_input_tokens",
)


def optional_json(path):
    try:
        return json.loads(path.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def evidence_path(value, base, output_root):
    if not isinstance(value, str) or not value:
        return None
    path = (base / value).resolve()
    return path if path.is_relative_to(output_root) and path.exists() else None


def evidence_link(path, label, run):
    href = quote(os.path.relpath(path, run), safe="/")
    return f'<a href="{escape(href)}">{escape(label)}</a>'


def finite_number(value):
    return type(value) in {int, float} and math.isfinite(value)


def case_result(task, run):
    name = task["name"]
    case = run / "cases" / name.rsplit("/", 1)[-1]
    outcome_path = case / "outcome.json"
    outcome = optional_json(outcome_path)
    if not isinstance(outcome, dict) or outcome.get("name") != name:
        status = task["status"]
        if status in {"ready", "needsNativeBuild"}:
            status = "missing" if case.is_dir() and any(case.iterdir()) else "unrun"
        return status, None, {}, case

    status = outcome.get("status")
    reward = outcome.get("reward")
    if status == "modelFailure":
        return status, 0, outcome, case
    if status == "scored" and finite_number(reward) and 0 <= reward <= 1:
        return status, reward, outcome, case
    if status in {"infraFailure", "blocked", "unresolved"}:
        return status, None, outcome, case
    return "missing", None, {**outcome, "reason": "结果状态或 reward 无效"}, case


def case_evidence(name, outcome, case, run):
    paths = {}
    summary_path = evidence_path(outcome.get("summaryPath") or "summary.json", case, run.parent)
    if summary_path and summary_path.is_file():
        paths["Talos 摘要"] = summary_path
        provenance = evidence_path("provenance.json", summary_path.parent, run.parent)
        if provenance and provenance.is_file():
            paths["任务与镜像来源"] = provenance
        captured = evidence_path("captured-submission/manifest.json", summary_path.parent, run.parent)
        if captured and captured.is_file():
            paths["工作区提交归档"] = captured
    runner_log = evidence_path(outcome.get("runnerLog") or "runner.log", case, run.parent)
    if runner_log and runner_log.is_file():
        paths["Runner 日志"] = runner_log
    for filename, label in (("buildkit.log", "构建日志"), ("buildkit-startup.log", "启动构建日志")):
        build_log = evidence_path(filename, case, run.parent)
        if build_log and build_log.is_file():
            paths[label] = build_log
    warm_build = evidence_path(outcome.get("warmBuildLog"), case, run.parent)
    if warm_build and warm_build.is_file():
        paths["镜像预热日志"] = warm_build
    outcome_path = evidence_path("outcome.json", case, run.parent)
    if outcome_path and outcome_path.is_file():
        paths["结果记录"] = outcome_path
    replay = evidence_path(outcome.get("regradeResult"), case, run.parent)
    if replay and replay.is_file():
        paths["Harbor 补评结果"] = replay
        for filename, label in (("test-stdout.txt", "补评输出"), ("ctrf.json", "补评 CTRF"),
                                ("reward_details.json", "补评分项")):
            log = evidence_path(filename, replay.parent / "verifier", run.parent)
            if log and log.is_file():
                paths[label] = log
        recipe = evidence_path(outcome.get("regradeRecipeDiff"), case, run.parent)
        if recipe and recipe.is_file():
            paths["补评镜像差异"] = recipe

    summary = optional_json(summary_path) if summary_path and summary_path.is_file() else None
    names = {name, name.rsplit("/", 1)[-1]}
    trial = next((item for item in (summary or {}).get("trials", []) if item.get("task") in names), {})
    if trial:
        for key, label in ARTIFACT_LABELS.items():
            path = evidence_path(trial.get("artifacts", {}).get(key), summary_path.parent, run.parent)
            if not path:
                continue
            if key == "verifier" and path.is_dir():
                files = {}
                for filename, file_label in (("test-stdout.txt", "Verifier 输出"), ("ctrf.json", "CTRF 测试结果")):
                    item = evidence_path(filename, path, run.parent)
                    if item and item.is_file():
                        files[file_label] = item
                if files:
                    paths.update(files)
                    continue
            paths[label] = path
    links = " ".join(evidence_link(path, label, run) for label, path in paths.items())
    return links or "—", trial


def attempt_history(case, run):
    entries = []
    for archived in sorted((run / "attempts").glob(f"*/{case.name}/outcome.json")):
        path = evidence_path(str(archived), run, run)
        if not path or not path.is_file():
            continue
        outcome = optional_json(path) or {}
        reason = outcome.get("reason")
        notes = [REASON_LABELS.get(reason, reason)] if isinstance(reason, str) and reason else []
        notes.extend(item for item in (outcome.get("details") or [])[:3] if isinstance(item, str))
        links = [evidence_link(path, archived.parents[1].name, run)]
        for filename, label in (("runner.log", "Runner 日志"), ("buildkit.log", "构建日志")):
            log = evidence_path(filename, path.parent, run)
            if log and log.is_file():
                links.append(evidence_link(log, label, run))
        summary = " · ".join(escape(item[:160]) for item in notes)
        entries.append(f"<li>{' '.join(links)} {summary}</li>")
    if not entries:
        return ""
    return f"<details><summary>历史尝试（{len(entries)}）</summary><ul>{''.join(entries)}</ul></details>"


def result_row(task, status, reward, outcome, case, run):
    name = task["name"]
    reasons = [item for item in task["reasons"] if isinstance(item, str)]
    reason = outcome.get("reason")
    if not reason and status == "missing":
        reason = "已有运行日志，待结果" if (case / "runner.log").is_file() else "已有运行文件，结果尚未归档"
    if isinstance(reason, str) and reason and reason not in reasons:
        reasons.append(REASON_LABELS.get(reason, reason))
    reasons.extend(item for item in (outcome.get("details") or []) if isinstance(item, str) and item not in reasons)
    links, trial = case_evidence(name, outcome, case, run)
    usage = trial.get("usage") or {}
    tokens = " / ".join(f"{usage[key]:,}" if finite_number(usage.get(key)) else "—" for key in TOKEN_KEYS)
    seconds = outcome.get("elapsedSeconds")
    elapsed = f"{seconds:,.1f} 秒" if finite_number(seconds) and seconds >= 0 else "—"
    observed = " · ".join(
        f"{label} {trial[key]:,.1f} 秒" for key, label in (
            ("observedModelSeconds", "模型"), ("observedToolSeconds", "工具"),
        ) if finite_number(trial.get(key))
    )
    if observed:
        elapsed += f'<div class="reason">{observed}</div>'
    regrade_seconds = outcome.get("regradeSeconds")
    if finite_number(regrade_seconds):
        elapsed += f'<div class="reason">补评 {regrade_seconds:,.1f} 秒</div>'
    score = f"{reward:g}" if reward is not None else "—"
    raw_reward = outcome.get("officialReward")
    if finite_number(raw_reward) and raw_reward != reward:
        score += f'<div class="reason">原始 verifier 记分 {raw_reward:g}（未计入本地得分）</div>'
    details = "<br>".join(escape(item) for item in reasons) or "—"
    digest = escape(task["digest"])
    return f"""<tr data-status="{status}" data-score="{reward if reward is not None else ''}">
      <th scope="row"><strong>{escape(name)}</strong><code class="digest">{digest}</code></th>
      <td class="preflight">{PREFLIGHT_LABELS[task['status']]}</td>
      <td><span class="status {status}">{RESULT_LABELS[status]}</span><div class="reason">{details}</div></td>
      <td>{score}</td><td>{elapsed}</td><td class="tokens">{tokens}</td><td class="links">{links}{attempt_history(case, run)}</td>
    </tr>"""


def render(run, inventory):
    tasks = inventory["tasks"]
    names = [task["name"] for task in tasks]
    if len(names) != len(set(names)) or len(names) > DATASET_SIZE:
        raise ValueError("The FrontierHarness inventory must contain at most 62 distinct tasks")
    counts = Counter()
    rewards = []
    rows = []
    for task in tasks:
        status, reward, outcome, case = case_result(task, run)
        counts[status] += 1
        if reward is not None:
            rewards.append(reward)
        rows.append(result_row(task, status, reward, outcome, case, run))

    scored = len(rewards)
    full = sum(reward == 1 for reward in rewards)
    mean = f"{sum(rewards) / scored:.3f}" if scored else "—"
    classified = sum(task["status"] != "unresolved" for task in tasks)
    admitted = sum(task["status"] in {"ready", "needsNativeBuild"} for task in tasks)
    finished = sum(counts[status] for status in ("scored", "modelFailure", "infraFailure"))
    coverage = (
        ("full", "满分", full),
        ("loss", "失分", scored - full),
        ("infra", "基础设施故障", counts["infraFailure"]),
        ("restricted", "未纳入", counts["blocked"] + counts["unresolved"]),
        ("pending", "待归档", DATASET_SIZE - finished - counts["blocked"] - counts["unresolved"]),
    )
    coverage_segments = "".join(
        f'<span class="{key}" style="flex-grow:{count}"></span>' for key, _, count in coverage if count
    )
    coverage_legend = "".join(
        f'<li><span class="swatch {key}" aria-hidden="true"></span>{label} <strong>{count}</strong></li>'
        for key, label, count in coverage
    )
    host = inventory.get("host") or {}
    resources = " · ".join(
        f"{host[key]:,} {label}" for key, label in (("cpus", "CPU"), ("memoryMb", "MiB"), ("gpus", "GPU"))
        if finite_number(host.get(key))
    )
    archive = evidence_path("attempts", run, run)
    history = " · " + evidence_link(archive, "历史尝试", run) if archive and archive.is_dir() else ""
    platform = escape(inventory["platform"]) + (" · " + resources if resources else "")
    configuration = optional_json(run / "run.json")
    run_metadata = ""
    if isinstance(configuration, dict):
        fields = (("model", "模型"), ("reasoningEffort", "推理强度"), ("runner", "Runner"), ("startedAt", "开始时间"))
        displayed = [f"{label} <code>{escape(configuration[key])}</code>" for key, label in fields
                     if isinstance(configuration.get(key), str) and configuration[key]]
        displayed.append(evidence_link(run / "run.json", "运行配置", run))
        run_metadata = '<p class="meta">' + " · ".join(displayed) + "</p>"
    status_options = "".join(f'<option value="{status}">{label}</option>' for status, label in RESULT_LABELS.items())
    return f"""<!doctype html>
<html lang="zh-CN">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>FrontierHarness v1.1 · 本地全量评测</title>
<style>
  :root {{ color-scheme: light; font-family: "Source Sans 3", "PingFang SC", sans-serif; color: #152536; background: #f5f7fa; }}
  body {{ margin: 0; }} main {{ max-width: 1440px; margin: auto; padding: 32px clamp(16px, 4vw, 56px) 48px; }}
  h1 {{ margin: 0 0 8px; font-family: "Instrument Serif", Georgia, "Songti SC", serif; font-size: clamp(26px, 4vw, 44px); line-height: 1.2; }}
  h2 {{ margin: 28px 0 12px; font-size: 22px; }} p {{ margin: 10px 0; line-height: 1.65; }}
  a {{ color: #125083; }} code {{ overflow-wrap: anywhere; }}
  .meta {{ color: #435466; }} .cards {{ display: grid; gap: 12px; grid-template-columns: repeat(auto-fit, minmax(165px, 1fr)); margin-top: 22px; }}
  .card {{ background: #fff; border: 1px solid #cbd7e0; border-radius: 10px; padding: 12px 16px; }}
  .card strong {{ display: block; margin: 4px 0; font-size: 26px; font-variant-numeric: tabular-nums; }}
  .coverage {{ margin: 22px 0 0; }} .coverage figcaption {{ margin-bottom: 8px; font-weight: 650; }}
  .coverage-bar {{ display: flex; height: 16px; overflow: hidden; border-radius: 3px; }}
  .coverage-bar span {{ flex-basis: 0; }} .coverage-bar span + span {{ border-left: 1px solid #fff; }}
  .swatch {{ display: inline-block; width: 9px; height: 9px; margin-right: 6px; border-radius: 2px; }}
  .full {{ background: #196951; }} .loss {{ background: #bb651e; }} .infra {{ background: #a03b49; }}
  .restricted {{ background: #607593; }} .pending {{ background: #c8d4e0; }}
  .counts {{ list-style: none; margin: 14px 0; padding: 0; display: flex; flex-wrap: wrap; gap: 8px 18px; }}
  .status {{ display: inline-block; border-radius: 4px; background: #e6edf3; padding: 2px 6px; white-space: nowrap; }}
  .scored {{ background: #d8eee8; color: #164f41; }} .modelFailure {{ background: #f9ebcd; color: #69420a; }}
  .infraFailure, .blocked {{ background: #f6e1e1; color: #782c31; }}
  .filters:not([hidden]) {{ display: flex; flex-wrap: wrap; align-items: center; gap: 8px 16px; margin: 0 0 12px; }}
  .filters input, .filters select {{ font: inherit; padding: 5px 8px; border: 1px solid #9aafc0; border-radius: 4px; background: #fff; }}
  .table-wrap {{ overflow-x: auto; background: #fff; border: 1px solid #cbd7e0; border-radius: 8px; }}
  table {{ width: 100%; border-collapse: collapse; text-align: left; font-size: 14px; }}
  th, td {{ padding: 10px; border-bottom: 1px solid #e3e9ee; vertical-align: top; }}
  .preflight {{ white-space: nowrap; }}
  thead {{ background: #eaf0f5; }} tbody tr:last-child > * {{ border-bottom: 0; }}
  tbody th {{ min-width: 200px; overflow-wrap: anywhere; }} .digest {{ display: block; font-size: 11px; font-weight: 400; color: #435466; }}
  .reason {{ margin-top: 4px; color: #435466; overflow-wrap: anywhere; }} .tokens {{ font-variant-numeric: tabular-nums; }}
  .links {{ min-width: 160px; }} .links a {{ display: inline-block; margin: 0 8px 4px 0; }}
</style>
<main>
  <h1>FrontierHarness v1.1 本地全量评测</h1>
  <p class="meta"><code>{escape(inventory['datasetRef'])}</code> · {platform} · <a href="preflight.json">原始预检清单</a>{history}</p>
  {run_metadata}
  <div class="cards">
    <div class="card">清单覆盖<strong>{len(tasks)} / {DATASET_SIZE}</strong><span class="meta">预检已分类 {classified} 题，本机纳入 {admitted} 题</span></div>
    <div class="card">已计分覆盖<strong>{scored} / {DATASET_SIZE}</strong></div>
    <div class="card">平均得分<strong>{mean}</strong><span class="meta">按已计分题目计算</span></div>
    <div class="card">本机已归档结果<strong>{finished} / {admitted}</strong><span class="meta">包含 {counts['infraFailure']} 题基础设施故障</span></div>
  </div>
  <figure class="coverage">
    <figcaption>全量清单覆盖分布</figcaption>
    <div class="coverage-bar" aria-hidden="true">{coverage_segments}</div>
    <ul class="counts">{coverage_legend}</ul>
  </figure>
  <p>模型失败和超时计 0 分，基础设施故障、环境受限和未执行项不计分。报告按每题当前归档结果统计。本地单轮成绩未达到官方每题四次计分的比较条件。 <a href="https://frontierharness.org/about/">官方评分规则</a></p>
  <h2>逐题结果</h2>
  <div class="filters" id="filters" hidden>
    <label>任务名称 <input id="task-search" type="search"></label>
    <label>结果状态 <select id="status-filter"><option value="">所有状态</option><option value="loss">失分题</option>{status_options}</select></label>
    <span id="visible-tasks" class="meta" aria-live="polite"></span>
  </div>
  <div class="table-wrap" role="region" aria-label="逐题结果" tabindex="0">
    <table id="results"><thead><tr><th>任务</th><th class="preflight">预检</th><th>结果和原因</th><th>得分</th><th>本次耗时</th><th>Token<br>输入 / 输出 / 缓存读 / 缓存写</th><th>证据</th></tr></thead>
    <tbody>{''.join(rows)}</tbody></table>
  </div>
</main>
<script>
  const search = document.getElementById("task-search");
  const statusFilter = document.getElementById("status-filter");
  const rows = Array.from(document.getElementById("results").tBodies[0].rows);
  function filterTasks() {{
    const query = search.value.trim().toLowerCase();
    const selected = statusFilter.value;
    let visible = 0;
    for (const row of rows) {{
      const name = row.cells[0].querySelector("strong").textContent.toLowerCase();
      const matches = selected === "loss" ? row.dataset.score !== "" && Number(row.dataset.score) < 1 :
        selected === "" || row.dataset.status === selected;
      row.hidden = !name.includes(query) || !matches;
      if (!row.hidden) visible++;
    }}
    document.getElementById("visible-tasks").textContent = "显示 " + visible + " / " + rows.length + " 题";
  }}
  search.addEventListener("input", filterTasks);
  statusFilter.addEventListener("change", filterTasks);
  document.getElementById("filters").hidden = false;
  filterTasks();
</script>
</html>
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_dir", type=Path, nargs="?", default=Path("output/issue-147-full"))
    run = parser.parse_args().run_dir.resolve()
    inventory = json.loads((run / "preflight.json").read_text())
    report = render(run, inventory)
    with NamedTemporaryFile(mode="w", encoding="utf-8", dir=run, delete=False) as temporary:
        temporary.write(report)
    destination = run / "index.html"
    Path(temporary.name).replace(destination)
    print(destination)


if __name__ == "__main__":
    main()
