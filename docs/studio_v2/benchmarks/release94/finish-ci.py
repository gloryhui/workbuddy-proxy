"""Wait for the original jobs, retry the one diagnosed failure once, collect."""
import datetime
import json
from pathlib import Path
import subprocess
import sys
import time

EVIDENCE = Path(__file__).resolve().parent
CI = EVIDENCE / "ci"
RUN = "41296054505"
SOURCE = "763f2aefb5e644d84250590aaa3bdc96a90b6e11"
FAILED_JOB = 124817029896
workspace = Path(json.loads((EVIDENCE / "artifact-workspace.json").read_text())["root"])
last = None
retry_started = False
deadline = time.monotonic() + 7200


def record(name, value):
    (CI / name).write_text(json.dumps(value, indent=2) + "\n")


while time.monotonic() < deadline:
    if (workspace / "stop-release-driver").exists():
        print("Stopped before any further action; no public release was made.", flush=True)
        sys.exit(0)
    query = subprocess.run([
        "gh", "run", "view", RUN, "--json", "databaseId,headSha,status,conclusion,attempt,jobs,url"
    ], capture_output=True, text=True, timeout=30)
    if query.returncode:
        record("retry-controller-observation-error.json", {"error": query.stderr[:300]})
        time.sleep(30)
        continue
    run = json.loads(query.stdout)
    assert run["headSha"] == SOURCE
    record("retry-controller-latest.json", run)
    summary = {"attempt": run["attempt"], "status": run["status"], "conclusion": run["conclusion"],
               "jobs": [{"id": j["databaseId"], "status": j["status"], "conclusion": j["conclusion"],
                         "active": [s["name"] for s in j["steps"] if s["status"] == "in_progress"]}
                        for j in run["jobs"] if j["conclusion"] != "skipped"]}
    if summary != last:
        print(json.dumps(summary), flush=True)
        last = summary
    if run["status"] != "completed":
        time.sleep(30)
        continue
    if run["conclusion"] == "success":
        record("desktop-success-before-collection.json", run)
        subprocess.run([sys.executable, str(EVIDENCE / "collect-artifacts.py")], check=True)
        print("Final packages collected and hash-verified. Awaiting source/report review before upload/publish.", flush=True)
        break
    assert not retry_started, "The single diagnosed retry also failed; do not retry again"
    assert run["attempt"] == 1, "Unexpected workflow attempt; do not dispatch an extra retry"
    failures = [j for j in run["jobs"] if j["conclusion"] not in {"success", "skipped"}]
    assert len(failures) == 1 and failures[0]["databaseId"] == FAILED_JOB
    assert [s["name"] for s in failures[0]["steps"] if s["conclusion"] == "failure"] == ["Run cargo test --locked"]
    diagnosis = json.loads((CI / "lock-budget-first-failure-diagnosis.json").read_text())
    assert diagnosis["failed_job"] == FAILED_JOB and diagnosis["local_single_test"]["passed"] == 1
    assert not (CI / "single-eligible-retry.json").exists(), "Preserve any earlier retry instead of duplicating it"
    record("desktop-original-terminal.json", run)
    retry = subprocess.run(["gh", "run", "rerun", RUN, "--job", str(FAILED_JOB)], capture_output=True, text=True)
    record("single-eligible-retry.json", {"at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                                         "original_job": FAILED_JOB, "source": SOURCE,
                                         "returncode": retry.returncode, "stdout": retry.stdout,
                                         "stderr": retry.stderr, "assertion_unchanged": True,
                                         "successful_jobs_not_rerun": True})
    assert retry.returncode == 0, "Eligible retry request failed; inspect response rather than bypassing it"
    retry_started = True
    print("Started the sole eligible retry of the original Mac ARM job; all assertions remain unchanged.", flush=True)
    time.sleep(30)
else:
    print("Observation deadline reached; this does not imply a terminal workflow.", flush=True)
