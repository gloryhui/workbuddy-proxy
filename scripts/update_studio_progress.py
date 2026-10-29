#!/usr/bin/env python3
"""Refresh the standalone Studio V2 timeline from QA, frame mapping and Git.

Run after each completed task, then commit the refreshed HTML and JSON.
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--task", type=int)
    parser.add_argument("--title")
    parser.add_argument("--status", choices=["planned", "developing", "validating", "recorded"])
    parser.add_argument("--description")
    parser.add_argument("--note")
    args = parser.parse_args()
    previous = json.loads(DATA.read_text(encoding="utf-8")) if DATA.exists() else {}
    current = dict(previous.get("current_task", {}))
    for key, value in {"id": args.task, "title": args.title, "status": args.status,
                       "description": args.description}.items():
        if value is not None:
            current[key] = value
    frames, timeline = frame_rows(), tasks()
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
                        **counts})
    report = {"version": 1, "updated_at": now, "project": "WorkBuddy Studio V2",
              "branch": git("branch", "--show-current"), "code_head": git("log", "-1", "--format=%H", "--", "gpui/src", "gpui/assets"),
              "phase": "开发中 · 分项实机验收穿插", "goal_status": "active",
              "counts": counts, "total_frames": len(frames), "recorded_tasks": len(recorded),
              "current_task": current, "latest_task": latest, "frames": frames,
              "timeline": timeline, "history": history[-200:],
              "verification": [
                  {"title": "40 帧最终视觉复核", "detail": f"当前 {counts['partial']} 帧有阶段性证据、{counts['reused']} 帧复用待核对。每帧仍按具体未完成项继续，部分通过不等于整体验收。"},
                  {"title": "键盘、IME 与无障碍", "detail": "已完成若干控件真实键盘路径；完整 Tab 顺序、中文输入法及辅助功能仍需统一复核。"},
                  {"title": "平台与生命周期", "detail": "macOS arm64 实机持续验证；Intel 目前只有类型检查，Windows、HiDPI 与完整托盘生命周期仍按矩阵保留待验。"},
                  {"title": "性能与证据口径", "detail": "保留真实新旧版本对比、核心/UI/合计 RSS、启动记录和请求 p50/p95。UI 就绪记录不是首帧或可交互时间。"}],
              "sources": [{"label": "40 帧映射", "href": "figma_node_map.md"},
                          {"label": "实施与 QA 原文", "href": "baseline_and_qa.md"},
                          {"label": "功能保护矩阵", "href": "feature_regression_matrix.md"}]}
    payload = json.dumps(report, ensure_ascii=False, indent=2).replace("</", "<\\/")
    html = PAGE.read_text(encoding="utf-8")
    pattern = r'(<script id="progress-data" type="application/json">)[\s\S]*?(</script>)'
    html, replaced = re.subn(pattern, lambda m: m[1] + "\n" + payload + "\n" + m[2], html, count=1)
    if replaced != 1:
        raise ValueError("HTML data marker missing; no report files updated")
    DATA.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    PAGE.write_text(html, encoding="utf-8")
    print(json.dumps({"html": str(PAGE), "counts": counts, "recorded_tasks": len(recorded),
                      "current_task": current}, ensure_ascii=False))


if __name__ == "__main__":
    main()
