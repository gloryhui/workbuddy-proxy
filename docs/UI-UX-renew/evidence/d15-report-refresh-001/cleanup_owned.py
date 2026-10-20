#!/usr/bin/env python3
"""Remove one exact, owned task CAP after its child processes have exited."""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path("/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp")
cap = Path(sys.argv[1])
report = Path(sys.argv[2])
assert cap.parent == ROOT and cap.name in {"d15-build-001", "d15-gui-001"}
assert not cap.is_symlink()
assert (cap / "owner.txt").read_text() == "Codex D15 2026-10-20\n"
refs = subprocess.run(["lsof", "+D", str(cap)], capture_output=True, text=True)
entries = sorted(cap.rglob("*"), key=lambda p: len(p.parts), reverse=True)
files = [{"path": str(p), "bytes": p.lstat().st_size} for p in entries if p.is_file() or p.is_symlink()]
data = {"cap": str(cap), "files": files, "bytes": sum(f["bytes"] for f in files), "references": refs.stdout, "removed": False}
report.write_text(json.dumps(data, indent=2))
if refs.stdout.strip():
    raise RuntimeError("CAP still has process references; retained")
for p in entries:
    if p.is_symlink() or p.is_file():
        p.unlink()
    else:
        p.rmdir()
cap.rmdir()
data["removed"] = True
report.write_text(json.dumps(data, indent=2))
