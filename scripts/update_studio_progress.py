#!/usr/bin/env python3
"""Refresh the standalone Studio V2 timeline from QA, frame mapping and Git.

Run after a complete feature or substantial milestone, then commit HTML and JSON.
Do not refresh for individual spacing edits, compiler runs, or intermediate builds.
The current task can be set with --task/--title/--status/--description.
"""
import argparse
import datetime as dt
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs/studio_v2"
DATA = DOCS / "progress_timeline.json"
PAGE = DOCS / "progress_timeline.html"


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True, encoding="utf-8").strip()


def plain(value):
    value = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", value)
    return re.sub(r"[`*]", "", value).strip()


def frame_rows():
    result = []
    for line in (DOCS / "figma_node_map.md").read_text(encoding="utf-8").splitlines():
        cells = [part.strip() for part in line.split("|")[1:-1]]
        if len(cells) != 4 or not re.fullmatch(r"\d+:\d+", cells[0]):
            continue
        node, title, source, detail = cells
        if detail == "未验收":
            status = "pending"
        elif detail.startswith("部分"):
            status = "partial"
        elif detail.startswith("共用"):
            status = "reused"
        elif detail.startswith("已验收"):
            status = "verified"
        else:
            raise ValueError(f"Unrecognized frame status for {node}: {detail}")
        result.append({"node": node, "title": title, "source": source,
                       "detail": detail, "status": status,
                       "url": "https://www.figma.com/design/zxDfIQ4lhNcTXrFT7qk8QZ?node-id=" + node.replace(":", "-")})
    if len(result) != 40:
        raise ValueError(f"Expected 40 mapped frames, found {len(result)}; report not updated")
    return result


def blame_lines():
    result = {}
    sha, number = None, None
    for line in git("blame", "--line-porcelain", "--", "docs/studio_v2/baseline_and_qa.md").splitlines():
        match = re.match(r"^([0-9a-f]{40}) \d+ (\d+)(?: \d+)?$", line)
        if match:
            sha, number = match[1], int(match[2])
        elif line.startswith("\t") and number is not None:
            result[number] = sha
    return result


def visual_review(frames):
    review = json.loads((DOCS / "visual_review.json").read_text(encoding="utf-8"))
    entries = review["frames"]
    ids = [entry["id"] for entry in entries]
    if len(ids) != len(set(ids)) or set(ids) != {frame["node"] for frame in frames}:
        raise ValueError("Visual review must cover the same 40 unique mapped frames; report not updated")
    statuses = ["not_reviewed", "reviewing", "reviewed_with_differences", "reviewed"]
    if any(entry["review_status"] not in statuses for entry in entries):
        raise ValueError("Unrecognized visual review status; report not updated")
    by_id = {entry["id"]: entry for entry in entries}
    for frame in frames:
        frame["visual_review"] = by_id[frame["node"]]
    return {"counts": {status: sum(entry["review_status"] == status for entry in entries)
                       for status in statuses}, "source": "visual_review.json",
            "meaning": review["meaning"]}


def tasks():
    source = (DOCS / "baseline_and_qa.md").read_text(encoding="utf-8")
    headers = list(re.finditer(r"^## 小功能\s+(\d+)：(.+)$", source, re.M))
    blame = blame_lines()
    result, commits = [], {}
    for i, header in enumerate(headers):
        body = source[header.end():headers[i+1].start() if i+1 < len(headers) else len(source)]
        paragraphs = re.findall(r"^- (.+?)(?=\n- |\n\n|\Z)", body, re.M | re.S)
        paragraphs = [plain(re.sub(r"\s+", " ", p)) for p in paragraphs]
        line_number = source[:header.start()].count("\n") + 1
        sha = blame.get(line_number, "0" * 40)
        committed = sha != "0" * 40
        if committed and sha not in commits:
            date, subject = git("show", "-s", "--format=%cI%n%s", sha).split("\n", 1)
            commits[sha] = {"date": date, "subject": subject}
        evidence = []
        for label, href in re.findall(r"\[([^\]]+)\]\(([^)]+)\)", body):
            if re.fullmatch(r"(?:benchmarks|screenshots)/[A-Za-z0-9_.-]+", href) and (DOCS / href).is_file():
                if not any(e["href"] == href for e in evidence):
                    evidence.append({"label": plain(label), "href": href})
        title = re.sub(r"（2026-.+?）$", "", header[2]).strip()
        limits = [p[:320] for p in paragraphs if re.search(r"未完成|尚未|仍待|不代表|未实际|不能.*完成", p)][-2:]
        result.append({"id": int(header[1]), "title": title,
                       "status": "recorded" if committed else "developing",
                       "summary": paragraphs[0][:220] if paragraphs else title,
                       "highlights": [p[:400] for p in paragraphs[:3]], "limits": limits,
                       "evidence": evidence[:8], "commit": sha if committed else None,
                       "date": commits.get(sha, {}).get("date"),
                       "commit_subject": commits.get(sha, {}).get("subject"),
                       "source": "baseline_and_qa.md"})
    return result



def release_plan(frames, timeline):
    """Validate the editable batch plan and attach only derived display fields."""
    plan = json.loads((DOCS / "release_plan.json").read_text(encoding="utf-8"))
    batches = plan["batches"]
    batch_ids = [batch["id"] for batch in batches]
    task_ids = [task["id"] for batch in batches for task in batch["tasks"]]
    mapped = {frame["node"]: frame for frame in frames}
    assigned = [node for batch in batches for node in batch["frames"]]
    if len(batch_ids) != len(set(batch_ids)) or len(task_ids) != len(set(task_ids)):
        raise ValueError("Duplicate release batch or task ID")
    if len(assigned) != len(set(assigned)) or set(assigned) != set(mapped):
        raise ValueError("Functional batches must own every mapped frame exactly once")
    if plan["current_known_frames"] != len(frames):
        raise ValueError("Release plan frame inventory is stale")
    known = set(batch_ids) | {plan["prerequisite"]["id"]}
    if plan["current_batch"] not in known or plan["prerequisite"]["status"] not in {"waiting", "passed"}:
        raise ValueError("Unknown current batch or prerequisite status")
    history = {task["id"] for task in timeline}
    allowed = {"planned", "developing", "validating", "passed", "blocked"}
    visited = set()
    def visit(batch_id, stack):
        if batch_id in stack:
            raise ValueError("Cyclic release batch dependencies")
        if batch_id in visited or batch_id == plan["prerequisite"]["id"]:
            return
        batch = next(item for item in batches if item["id"] == batch_id)
        for dependency in batch["depends_on"]:
            if dependency not in known:
                raise ValueError("Unknown release batch dependency: " + dependency)
            visit(dependency, stack | {batch_id})
        visited.add(batch_id)
    for batch in batches:
        visit(batch["id"], set())
        if batch["status"] not in allowed or batch["kind"] not in {"feature", "release"}:
            raise ValueError("Unrecognized release batch status/kind")
        accepted = batch["accepted_frames"]
        if len(accepted) != len(set(accepted)) or not set(accepted) <= set(batch["frames"]):
            raise ValueError("Accepted frames must be unique members of their batch")
        verified = {node for node in batch["frames"] if mapped[node]["status"] == "verified"}
        if set(accepted) != verified:
            raise ValueError("Release plan and frame mapping disagree about accepted frames")
        for task in batch["tasks"]:
            if task["acceptance"] not in {"pending", "passed", "blocked"}:
                raise ValueError("Unknown release task acceptance state")
            if not set(task["history_tasks"]) <= history:
                raise ValueError("Release task cites unknown historical QA IDs")
        if batch["status"] == "passed" and (
            any(task["acceptance"] != "passed" for task in batch["tasks"])
            or not batch["acceptance_evidence"]
            or set(accepted) != set(batch["frames"])
        ):
            raise ValueError("Passed batch requires all tasks, frames and current acceptance evidence")
        if batch["status"] == "passed":
            states = {item["id"]: item["status"] for item in batches}
            states[plan["prerequisite"]["id"]] = plan["prerequisite"]["status"]
            if any(states[dependency] != "passed" for dependency in batch["depends_on"]):
                raise ValueError("Passed batch has an incomplete prerequisite")
        for href in batch["acceptance_evidence"]:
            if not re.fullmatch(r"(?:benchmarks|screenshots)/[A-Za-z0-9_.-]+", href) or not (DOCS / href).is_file():
                raise ValueError("Missing or unsafe batch acceptance evidence")
        batch["frame_details"] = [{"id": node, "title": mapped[node]["title"],
                                    "status": mapped[node]["status"],
                                    "review_status": mapped[node]["visual_review"]["review_status"]}
                                   for node in batch["frames"]]
        batch["reviewed_frames"] = sum(item["review_status"] != "not_reviewed" for item in batch["frame_details"])
    protected = [line.split("|")[1].strip() for line in (DOCS / "feature_regression_matrix.md").read_text(encoding="utf-8").splitlines()
                 if line.startswith("| ") and line.split("|")[1].strip() not in {"功能", "---"}]
    coverage = plan["protection_coverage"]
    if len(coverage) != len(protected) or {item["feature"] for item in coverage} != set(protected):
        raise ValueError("Release plan must cover every protected feature exactly once")
    if any(not item["tasks"] or not set(item["tasks"]) <= set(task_ids) for item in coverage):
        raise ValueError("Protected feature points to unknown or empty acceptance tasks")
    plan["metrics"] = {"functional_batches": sum(item["kind"] == "feature" for item in batches),
                       "release_batches": sum(item["kind"] == "release" for item in batches),
                       "passed_batches": sum(item["status"] == "passed" for item in batches),
                       "tasks": len(task_ids),
                       "passed_tasks": sum(task["acceptance"] == "passed" for item in batches for task in item["tasks"])}
    return plan


def plan_markdown(plan):
    lines = ["# Studio V2 正式版批次计划", "", "本文件由 release_plan.json 生成；修改计划数据后运行 scripts/update_studio_progress.py。", "",
             "当前批次：" + plan["current_batch"] + "；Goal：" + plan.get("goal_status", "unknown") + "。主体已有实现和分项证据，界面与正式版验收以实际签收为准。", "",
             "## 口径与顺序", "", *["- " + value for value in plan["acceptance_model"].values()], "",
             "执行顺序：P00 → B01 → B02 → B03 → B04 → B05 → R01 → R02 / R03 → R04。", "",
             "## 批量推进规则", "", *["- " + rule for rule in plan["working_rules"]], "",
             "## P00：" + plan["prerequisite"]["title"], "", plan["prerequisite"]["reason"], "",
             *[str(index) + ". " + step for index, step in enumerate(plan["prerequisite"]["steps"], 1)], "",
             "通过标准：" + plan["prerequisite"]["exit"], ""]
    for batch in plan["batches"]:
        lines += ["## " + batch["id"] + "：" + batch["title"], "",
                  "覆盖画板：" + ("、".join(batch["frames"]) or "全局发布门禁，不新增画板计数") + "。", "",
                  "已有成果：" + batch["completed_summary"], "", "本批交付：" + batch["delivery"], ""]
        for task in batch["tasks"]:
            lines += ["### " + task["id"] + " " + task["title"], "",
                      "- 已完成/已有证据：" + task["completed"],
                      "- 剩余工作：" + task["remaining"],
                      "- 验收标准：" + task["acceptance_check"],
                      "- 历史记录：" + ("、".join("UI-" + str(number) for number in task["history_tasks"]) or "按实际最终候选补证，不冒用历史通过") + "。", ""]
    lines += ["## 完整功能保护项与验收任务映射", "", "| 现有功能保护项 | 对应任务 |", "| --- | --- |",
              *["| " + item["feature"] + " | " + "、".join(item["tasks"]) + " |" for item in plan["protection_coverage"]], ""]
    lines += ["## 排程说明", "", plan["schedule"]["type"] + "。", "", plan["schedule"]["target"], "", plan["schedule"]["estimate_policy"], ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--task", type=int)
    parser.add_argument("--title")
    parser.add_argument("--status", choices=["planned", "developing", "validating", "recorded"])
    parser.add_argument("--description")
    parser.add_argument("--note")
    parser.add_argument("--goal-status", choices=["active", "paused", "blocked", "complete"],
                        help="Last observed Goal panel state; records it without controlling Goal")
    args = parser.parse_args()
    previous = json.loads(DATA.read_text(encoding="utf-8")) if DATA.exists() else {}
    frames, timeline = frame_rows(), tasks()
    current = dict(previous.get("current_task", {}))
    if args.task is not None and args.task != current.get("id"):
        selected = next((task for task in timeline if task["id"] == args.task), None)
        current = {"id": args.task,
                   "title": selected["title"] if selected else f"任务 {args.task}",
                   "status": selected["status"] if selected else "planned",
                   "description": selected["summary"] if selected else ""}
    for key, value in {"id": args.task, "title": args.title, "status": args.status,
                       "description": args.description}.items():
        if value is not None:
            current[key] = value
    review = visual_review(frames)
    plan = release_plan(frames, timeline)
    plan["goal_status"] = args.goal_status or previous.get("goal_status", "unknown")
    if current.get("status") == "recorded" and not any(
        task["id"] == current.get("id") and task["commit"] for task in timeline
    ):
        raise ValueError("A recorded task must have a committed QA section; report not updated")
    counts = {status: sum(f["status"] == status for f in frames)
              for status in ["pending", "partial", "reused", "verified"]}
    recorded = [t for t in timeline if t["commit"]]
    latest = max(recorded, key=lambda t: t["id"]) if recorded else None
    now = dt.datetime.now(dt.timezone(dt.timedelta(hours=8))).isoformat(timespec="seconds")
    history = list(previous.get("history", []))
    previous_counts = {key: previous.get("counts", {}).get(key, 0) for key in counts}
    if args.note or not history or previous_counts != counts or previous.get("current_task") != current:
        history.append({"date": now, "note": args.note or "同步当前任务与画板验收状态",
                        **counts, "remaining_final": len(frames) - counts["verified"],
                        "visual_review": review["counts"]})
    report = {"version": 2, "updated_at": now, "project": "WorkBuddy Studio V2",
              "branch": git("branch", "--show-current"), "code_head": git("log", "-1", "--format=%H", "--", "gpui/src", "gpui/assets"),
              "phase": ("正式版批次规划 · 等待Figma连接" if plan["goal_status"] == "paused" and plan["prerequisite"]["status"] == "waiting"
                        else "界面终验记录齐备 · 总体验收待核对" if counts["verified"] == len(frames)
                        else "批次推进 · " + plan["current_batch"] if plan["goal_status"] == "active"
                        else "专项验收与差异修复 · 全量逐帧终验未完成"),
              "goal_status": args.goal_status or previous.get("goal_status", "unknown"),
              "counts": counts, "total_frames": len(frames), "recorded_tasks": len(recorded),
              "remaining_final": len(frames) - counts["verified"], "visual_review": review,
              "current_task": current, "latest_task": latest, "frames": frames,
              "functional_modules": [{"title": batch["title"], "implemented": batch["completed_summary"],
                                      "pending": batch["delivery"],
                                      "tasks": sorted({number for task in batch["tasks"] for number in task["history_tasks"]})}
                                     for batch in plan["batches"] if batch["kind"] == "feature"],
              "release_plan": plan,
              "timeline": timeline, "history": history[-200:],
              "verification": [{"title": batch["title"], "detail": batch["delivery"]}
                               for batch in plan["batches"] if batch["kind"] == "release"],
              "sources": [{"label": "正式版批次计划", "href": "release_plan.md"},
                          {"label": "可维护计划数据", "href": "release_plan.json"},
                          {"label": "40 帧映射", "href": "figma_node_map.md"},
                          {"label": "实施与 QA 原文", "href": "baseline_and_qa.md"},
                          {"label": "逐帧视觉审查台账", "href": "visual_review.json"},
                          {"label": "功能保护矩阵", "href": "feature_regression_matrix.md"}]}
    payload = json.dumps(report, ensure_ascii=False, indent=2).replace("</", "<\\/")
    html = PAGE.read_text(encoding="utf-8")
    pattern = r'(<script id="progress-data" type="application/json">)[\s\S]*?(</script>)'
    html, replaced = re.subn(pattern, lambda m: m[1] + "\n" + payload + "\n" + m[2], html, count=1)
    if replaced != 1:
        raise ValueError("HTML data marker missing; no report files updated")
    (DOCS / "release_plan.md").write_text(plan_markdown(plan), encoding="utf-8")
    DATA.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    PAGE.write_text(html, encoding="utf-8")
    print(json.dumps({"html": str(PAGE), "counts": counts, "recorded_tasks": len(recorded),
                      "current_task": current}, ensure_ascii=False))


if __name__ == "__main__":
    main()
