"""Collect one successful frozen-source release run without rerunning checks."""
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import subprocess
import time
import zipfile

SOURCE = "763f2aefb5e644d84250590aaa3bdc96a90b6e11"
RUNS = (41296054368, 41296054505)
REPO = "gloryhui/workbuddy-proxy"
EVIDENCE = Path(__file__).resolve().parent
PROJECT = EVIDENCE.parents[3]


def gh(*args):
    for attempt in (1, 2, 3):
        result = subprocess.run(["gh", *args], capture_output=True, text=True, timeout=35)
        if result.returncode == 0:
            return json.loads(result.stdout)
        transient = any(word in result.stderr.lower()
                        for word in ("eof", "connection reset", "timed out", "timeout"))
        with (EVIDENCE / "ci" / "metadata-read-transport.jsonl").open("a") as output:
            output.write(json.dumps({"args": args, "attempt": attempt,
                                     "returncode": result.returncode, "transient": transient}) + "\n")
        if not transient or attempt == 3:
            raise RuntimeError("Metadata read failed; preserve workflow and inspect transport/access")
        time.sleep(2)


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def collect(item, reports_only=False):
    workspace, artifact, delivery = item
    assert artifact["workflow_run"]["head_sha"] == SOURCE
    assert not artifact["expired"]
    archive = workspace / (str(artifact["id"]) + ".zip")
    reused = archive.exists()
    if reused:
        assert archive.is_file() and not archive.is_symlink()
        assert archive.stat().st_uid == os.getuid()
    else:
        observations = []
        for attempt in (1, 2):
            staging = workspace / f"{artifact['id']}-{time.time_ns()}.partial"
            with staging.open("xb") as handle:
                request = subprocess.run([
                    "gh", "api", "--allow-escape-sequences",
                    f"repos/{REPO}/actions/artifacts/{artifact['id']}/zip"
                ], stdout=handle, stderr=subprocess.PIPE)
            received = digest(staging)
            observations.append({"attempt": attempt, "returncode": request.returncode,
                                 "bytes": staging.stat().st_size, "sha256": received,
                                 "verified": artifact.get("digest") == "sha256:" + received})
            (EVIDENCE / "ci" / f"{artifact['id']}-download-observations.json").write_text(
                json.dumps(observations, indent=2) + "\n")
            if request.returncode == 0 and observations[-1]["verified"]:
                assert not archive.exists()
                staging.rename(archive)
                break
        else:
            raise RuntimeError("Artifact download failed twice; preserve partial files and inspect transport")
    actual = digest(archive)
    expected = artifact.get("digest", "")
    assert expected == "sha256:" + actual, "Artifact server digest mismatch"
    result = {"artifact": artifact, "download_sha256": actual,
              "immutable_download_reused": reused, "packages": [], "reports": []}
    platform = artifact["name"].removeprefix("workbuddy-proxy-")
    with zipfile.ZipFile(archive) as package:
        assert package.testzip() is None
        for member in package.infolist():
            relative = PurePosixPath(member.filename.replace("\\", "/"))
            assert not relative.is_absolute() and ".." not in relative.parts
            assert not relative.parts or ":" not in relative.parts[0]
            assert not stat.S_ISLNK(member.external_attr >> 16)
            if member.is_dir():
                continue
            name = relative.name
            expected_packages = {
                "workbuddy-proxy-0.6.0-macos-arm64.dmg",
                "workbuddy-proxy-0.6.0-macos-x64.dmg",
                "workbuddy-proxy-0.6.0-windows-x64.zip",
                "workbuddy-proxy-0.6.0-windows-x64-setup.exe",
            }
            is_package = relative.parts[0] == "dist" and name in expected_packages
            if reports_only and is_package:
                continue
            is_report = name.endswith(".json") and (
                relative.parts[0] == "bench-results"
                or "workbuddy-import-validation" in relative.parts
            )
            is_native_image = name.endswith(".png") and "windows-native" in relative.parts
            if not (is_package or is_report or is_native_image):
                continue
            target = delivery / name if is_package else EVIDENCE / "ci" / platform / Path(*relative.parts)
            if target.exists():
                assert target.is_file() and not target.is_symlink()
                assert target.stat().st_uid == os.getuid()
                assert digest(target) == hashlib.sha256(package.read(member)).hexdigest(), "Existing bytes differ"
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with package.open(member) as source, target.open("xb") as output:
                    shutil.copyfileobj(source, output)
            entry = {"path": str(target.relative_to(PROJECT)), "bytes": target.stat().st_size,
                     "sha256": digest(target), "archive_member": member.filename}
            result["packages" if is_package else "reports"].append(entry)
    return result


def main():
    config = json.loads((EVIDENCE / "artifact-workspace.json").read_text())
    workspace = Path(config["root"])
    owner = json.loads((workspace / "ownership.json").read_text())
    assert workspace.resolve() == workspace and owner["root"] == str(workspace)
    assert owner["created_by_this_session"] and owner["uid"] == os.getuid()
    runs = [gh("run", "view", str(run), "--json", "databaseId,headSha,status,conclusion,jobs,url")
            for run in RUNS]
    assert all(run["headSha"] == SOURCE and run["status"] == "completed"
               and run["conclusion"] == "success" for run in runs)
    metadata = gh("api", f"repos/{REPO}/actions/runs/{RUNS[1]}/artifacts?per_page=100")
    expected = {"workbuddy-proxy-windows-x64", "workbuddy-proxy-macos-arm64", "workbuddy-proxy-macos-x64"}
    artifacts = [artifact for artifact in metadata["artifacts"] if artifact["name"] in expected]
    assert len(artifacts) == 3 and {artifact["name"] for artifact in artifacts} == expected
    (EVIDENCE / "ci" / "terminal-runs.json").write_text(json.dumps(runs, indent=2) + "\n")
    (EVIDENCE / "ci" / "artifacts.json").write_text(json.dumps(metadata, indent=2) + "\n")
    delivery = PROJECT / config["permanent_delivery"]
    assert delivery.resolve() == delivery and delivery.is_relative_to(PROJECT)
    ownership = EVIDENCE / "delivery-ownership.json"
    if delivery.exists():
        known = json.loads(ownership.read_text())
        assert known == {"path": str(delivery), "source": SOURCE, "uid": os.getuid(), "created_by_this_session": True}
        assert all(path.name.startswith("workbuddy-proxy-0.6.0-") or path.name == "SHA256SUMS.txt"
                   for path in delivery.iterdir())
    else:
        delivery.mkdir(parents=True)
        ownership.write_text(json.dumps({"path": str(delivery), "source": SOURCE,
                                        "uid": os.getuid(), "created_by_this_session": True}, indent=2) + "\n")
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        results = list(pool.map(collect, [(workspace, artifact, delivery) for artifact in artifacts]))
    packages = sorted([item for result in results for item in result["packages"]], key=lambda item: item["path"])
    assert len(packages) == 4 and len({Path(item["path"]).name for item in packages}) == 4
    checksums = "".join(f"{item['sha256']}  {Path(item['path']).name}\n" for item in packages)
    checksum_file = delivery / "SHA256SUMS.txt"
    if checksum_file.exists():
        assert checksum_file.read_text() == checksums
    else:
        checksum_file.write_text(checksums)
    proof = {"source": SOURCE, "Core_run": RUNS[0], "Desktop_run": RUNS[1],
             "artifacts": results, "packages": packages,
             "checksums": str((delivery / "SHA256SUMS.txt").relative_to(PROJECT)),
             "checksums_sha256": digest(delivery / "SHA256SUMS.txt")}
    (EVIDENCE / "final-packages.json").write_text(json.dumps(proof, indent=2) + "\n")
    print(json.dumps({"source": SOURCE, "packages": packages, "checksums": proof["checksums"]}), flush=True)


if __name__ == "__main__":
    main()
