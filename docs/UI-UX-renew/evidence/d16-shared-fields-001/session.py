#!/usr/bin/env python3
"""Supervise a synthetic core/GUI fixture; all UI actions are performed via CUA."""
import os
import sys
from pathlib import Path

ROOT = Path("/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign")
EV = Path(__file__).resolve().parent
CAP = Path("/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/d16-gui-003")
assert not CAP.exists()
CAP.mkdir(parents=True)
(CAP / "owner.txt").write_text("Codex D16 2026-10-20\n")
os.environ.update(TMPDIR=str(CAP), TMP=str(CAP), TEMP=str(CAP))
sys.dont_write_bytecode = True

import gc
import hashlib
import importlib.util
import json
import signal
import shutil
import plistlib
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

for name in ("logs", "shots", "ax"):
    (EV / name).mkdir(exist_ok=True)
children = []
server = None
thread = None
config = CAP / "run/config"

class Models(BaseHTTPRequestHandler):
    def do_GET(self):
        with (EV / "logs/model-requests.txt").open("a") as log:
            log.write(f"{time.time():.3f} GET {self.path}\n")
        if self.path.rstrip("/") not in ("/v1/models", "/models"):
            self.send_error(404)
            return
        body = json.dumps({"object": "list", "data": [
            {"id": "mock-model-a", "object": "model"},
            {"id": "mock-model-b", "object": "model"}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass

def interrupted(*_):
    raise KeyboardInterrupt()

for sig in (signal.SIGINT, signal.SIGTERM):
    signal.signal(sig, interrupted)

try:
    server = HTTPServer(("127.0.0.1", 0), Models)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    spec = importlib.util.spec_from_file_location("d16_fixture", ROOT / "docs/UI-UX-renew/evidence/d01-icons-001/fixture.py")
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    fixture.EV, fixture.CAP, fixture.RUN = EV, CAP, CAP / "run"
    original_write = fixture.write_config

    def write_config(path):
        original_write(path)
        payload = json.loads((path / "config.json").read_text())
        for row, protocol in zip(payload["providers"], ("openai_chat", "openai_responses", "anthropic_messages", "gemini_generate_content")):
            row["protocol"] = protocol
            row["upstream"] = f"http://127.0.0.1:{server.server_port}/v1"
            row["api_key"] = "sk-test-local"
        (path / "config.json").write_text(json.dumps(payload))

    fixture.write_config = write_config
    for name in ("start_mock", "start_core"):
        original = getattr(fixture, name)
        def tracked(*args, original=original):
            process, port = original(*args)
            children.append(process)
            return process, port
        setattr(fixture, name, tracked)
    fixture.setup()
    gc.collect()  # sqlite connection context managers commit but do not close.
    state = json.loads((CAP / "run/run.json").read_text())
    bundle = CAP / "WorkBuddy Proxy D16.app"
    macos = bundle / "Contents/MacOS"
    macos.mkdir(parents=True)
    binary = macos / "workbuddy-proxy-gpui"
    shutil.copy2(ROOT / "gpui/target/release/workbuddy-proxy-gpui", binary)
    (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps({
        "CFBundleExecutable": binary.name,
        "CFBundleIdentifier": "com.workbuddyproxy.codex.d16",
        "CFBundleName": "WorkBuddy Proxy D16",
        "CFBundleDisplayName": "WorkBuddy Proxy D16",
        "CFBundlePackageType": "APPL",
        "CFBundleVersion": "1",
        "NSHighResolutionCapable": True,
    }))
    # Only the fixture's fake config/home are passed to the GUI.
    with (EV / "logs/gui-stdout.txt").open("wb") as out, (EV / "logs/gui-stderr.txt").open("wb") as err:
        gui = subprocess.Popen(
            [str(binary), "--config-dir", str(config)],
            env=fixture.env_for(config, state["mock_port"]), cwd=ROOT, stdout=out, stderr=err)
        children.append(gui)
    state["gpui_pid"] = gui.pid
    state["bundle"] = str(bundle)
    state["config_sha_before"] = hashlib.sha256((config / "config.json").read_bytes()).hexdigest()
    (EV / "logs/runtime.json").write_text(json.dumps(state, indent=2))
    print("READY " + json.dumps({"gui_pid": gui.pid, "config": str(config)}), flush=True)
    while not (CAP / "stop").exists():
        if gui.poll() is not None:
            raise RuntimeError(f"GUI exited early: {gui.returncode}")
        time.sleep(0.25)
finally:
    if config.exists() and (config / "config.json").exists():
        (EV / "logs/config-sha-final.txt").write_text(hashlib.sha256((config / "config.json").read_bytes()).hexdigest() + "\n")
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
    for child in reversed(children):
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
    if server is not None:
        server.shutdown()
        server.server_close()
    if thread is not None:
        thread.join(timeout=5)
    run = CAP / "run"
    for source, name in ((run / "core.log", "core.txt"), (run / "mock.log", "catalog.txt")):
        if source.exists():
            (EV / "logs" / name).write_bytes(source.read_bytes())
    gc.collect()
    subprocess.run([sys.executable, str(EV / "cleanup_owned.py"), str(CAP), str(EV / "logs/cleanup-gui.json")], check=True)
