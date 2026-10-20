#!/usr/bin/env python3
"""10-cycle X05 repeat probe on an isolated GUI. Read-only interactions."""
import csv, hashlib, json, os, subprocess, sys, time
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t09-x05-repeat-001'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t09-x05-repeat-001')
RUN = CAP / 'run'
AX = CAP / 'bin/axdrive'
PID = int((CAP / 'gpui.pid').read_text())
WID = int((CAP / 'wid').read_text())
DB = RUN / 'config/usage.sqlite3'
CFG = RUN / 'config/config.json'

def sh(cmd, timeout=20):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)

def ax(*args, timeout=20):
    return sh([str(AX), *map(str, args)], timeout=timeout).stdout

def click(x, y):
    ax('click', x, y)

def key(code):
    sh(['osascript', '-e', 'tell application "System Events" to key code %d' % code])

def escape():
    ax('escape', PID)

def scroll(x, y, dy):
    ax('scroll', x, y, dy)

def shot(name):
    ax('shot', WID, str(EV / 'shots' / name))

def dump(name):
    ax('dump', PID, str(EV / 'ax' / name))

def ps_metrics(pid):
    out = sh(['ps', '-o', 'rss=,cputime=', '-p', str(pid)]).stdout.strip()
    if not out:
        return {'rss_kb': None, 'cpu_s': None}
    rss, cpu = out.split(None, 1)
    parts = cpu.split(':')
    sec = 0
    for p in parts:
        sec = sec * 60 + float(p)
    return {'rss_kb': int(rss), 'cpu_s': round(sec, 2)}

def fd_count(pid):
    out = sh(['lsof', '-p', str(pid)]).stdout.splitlines()
    return max(0, len(out) - 1)

def thread_count(pid):
    out = sh(['ps', '-M', str(pid)]).stdout.splitlines()
    return max(0, len(out) - 1)

def window_count():
    try:
        data = json.loads(ax('windows', PID))
        return len(data)
    except Exception:
        return None

def mock_requests():
    p = CAP / 'logs/mock-provider.log'
    return len(p.read_text().splitlines()) if p.exists() else 0

def db_counts():
    import sqlite3
    with sqlite3.connect(DB) as c:
        return (c.execute('select count(*) from usage_requests').fetchone()[0],
                c.execute('select count(*) from usage_costs').fetchone()[0])

def cfg_hash():
    return hashlib.sha256(CFG.read_bytes()).hexdigest()

def snapshot(label, extra=None):
    gui = ps_metrics(PID)
    core_pid = json.loads((RUN / 'run.json').read_text())['core_pid']
    core = ps_metrics(core_pid)
    rows, costs = db_counts()
    rec = {
        'label': label, 'ts': time.time(),
        'gui_rss_kb': gui['rss_kb'], 'gui_cpu_s': gui['cpu_s'],
        'core_rss_kb': core['rss_kb'], 'core_cpu_s': core['cpu_s'],
        'gui_fd': fd_count(PID), 'core_fd': fd_count(core_pid),
        'gui_threads': thread_count(PID), 'core_threads': thread_count(core_pid),
        'windows': window_count(), 'mock_requests': mock_requests(),
        'rows': rows, 'costs': costs, 'config_sha256': cfg_hash(),
    }
    if extra:
        rec.update(extra)
    return rec

def main():
    cycles = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    # normalise the open filter menu: choose "all" then close
    click(680, 220); time.sleep(0.2); escape(); time.sleep(0.3)
    rows = []
    base = snapshot('initial')
    rows.append(base)
    shot('10-initial.png'); dump('10-initial.txt')
    for i in range(1, cycles + 1):
        stage = {}
        t0 = time.monotonic()
        # A: provider switch (opencode) from stats
        click(127, 248); time.sleep(0.35); stage['provider_A_s'] = round(time.monotonic() - t0, 3)
        # B: protocol dialog open/close
        t = time.monotonic(); click(1051, 330); time.sleep(0.25); escape(); time.sleep(0.2)
        stage['protocol_s'] = round(time.monotonic() - t, 3)
        # C: model picker via loopback mock, open/close (no add)
        t = time.monotonic(); click(961, 462); time.sleep(0.8)
        picker_path = EV / 'ax' / '_picker-check.txt'
        ax('dump', PID, str(picker_path), timeout=20)
        picker = picker_path.read_text()
        if 'mock-model-a' not in picker and 'mock-model' not in picker:
            raise RuntimeError('picker did not show mock models on cycle %d' % i)
        escape(); time.sleep(0.2); stage['picker_s'] = round(time.monotonic() - t, 3)
        # D: supplier B then back
        t = time.monotonic(); click(127, 284); time.sleep(0.25); click(127, 248); time.sleep(0.25)
        stage['provider_B_A_s'] = round(time.monotonic() - t, 3)
        # E: stats + status filter change/reset
        t = time.monotonic(); click(127, 151); time.sleep(0.45)
        click(698, 216); time.sleep(0.2); click(680, 265); time.sleep(0.25)
        click(698, 216); time.sleep(0.2); click(680, 220); time.sleep(0.25); escape(); time.sleep(0.2)
        stage['stats_filter_s'] = round(time.monotonic() - t, 3)
        # F: request log scroll down/up
        t = time.monotonic(); scroll(1400, 700, -700); time.sleep(0.2); scroll(1400, 700, 700); time.sleep(0.2)
        stage['scroll_s'] = round(time.monotonic() - t, 3)
        rec = snapshot('cycle-%02d' % i, stage)
        rec['cycle_s'] = round(time.monotonic() - t0, 3)
        rows.append(rec)
        if i in (1, 5, 10):
            shot('11-cycle-%02d.png' % i); dump('11-cycle-%02d.txt' % i)
    rows.append(snapshot('final'))
    shot('12-final.png'); dump('12-final.txt')
    fields = list(rows[0].keys())
    for r in rows:
        for k in r:
            if k not in fields:
                fields.append(k)
    with open(EV / 'logs/measurements.csv', 'w', newline='') as f:
        w = csv.DictWriter(f, fieldnames=fields); w.writeheader(); w.writerows(rows)
    with open(EV / 'logs/measurements.json', 'w') as f:
        json.dump(rows, f, ensure_ascii=False, indent=2)
    print(json.dumps({'initial': rows[0], 'final': rows[-1], 'cycles': cycles}, ensure_ascii=False, indent=2))

if __name__ == '__main__':
    main()
