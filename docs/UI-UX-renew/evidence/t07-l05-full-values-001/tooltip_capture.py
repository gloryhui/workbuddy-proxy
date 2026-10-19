#!/usr/bin/env python3
"""Hover-tooltip capture for UI-T07-L05-FULL-VALUES-001.

Both builds put the same complete-value tooltip on the token cell and the cost
cell, but the main run's OCR label search is unreliable on the current build's
two-line token cell. This script anchors the hover geometrically instead: the
first AX row slot plus the token/cost column x range, so both builds hover the
same row and the same columns.

usage: tooltip_capture.py BINARY LABEL
"""
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l05-full-values-001'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/'
           '01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l05-full-values-001')
RUN = CAP / 'run'
AX = CAP / 'bin/axdrive'
OCR = CAP / 'bin/ocr'
OUT = EV / 'raw' / sys.argv[2]
SHOTS = OUT / 'shots'
AXS = OUT / 'ax'
CONFIG = Path(json.loads((RUN / 'run.json').read_text())['config'])
BINARY = Path(sys.argv[1])
LABEL = sys.argv[2]


def log(message):
    with open(OUT / 'tooltip.log', 'a', encoding='utf-8') as handle:
        handle.write('%s %s\n' % (time.strftime('%H:%M:%S'), message))
    print(message, flush=True)


def call(*args):
    return subprocess.run([str(a) for a in args], capture_output=True,
                          text=True, timeout=120)


def ax(*args):
    return call(AX, *args)


def env_for(config):
    env = dict(os.environ)
    env.update({
        'HOME': str(RUN / 'home'), 'TMPDIR': str(RUN / 'tmp'),
        'TMP': str(RUN / 'tmp'), 'TEMP': str(RUN / 'tmp'),
        'WORKBUDDY_PROXY_CONFIG_DIR': str(config),
        'XDG_CONFIG_HOME': str(RUN / 'home/.config'),
        'XDG_DATA_HOME': str(RUN / 'home/.local/share'),
        'XDG_CACHE_HOME': str(RUN / 'home/.cache'),
    })
    return env


def rows_of(path):
    rows = []
    for line in Path(path).read_text(encoding='utf-8', errors='replace').splitlines():
        if 'AXRow' not in line:
            continue
        try:
            frame = line.split('[')[1].split(']')[0]
            origin, size = frame.split(' ')
            x, y = origin.split(',')
            w, h = size.split('x')
            rows.append({'x': int(x), 'y': int(y), 'w': int(w), 'h': int(h)})
        except (IndexError, ValueError):
            continue
    rows.sort(key=lambda row: row['y'])
    return rows


def snapshot(name):
    png = SHOTS / (name + '.png')
    ax('shot', WINDOW, png)
    lines = json.loads(call(OCR, png, 'zh-Hans,en-US').stdout or '[]')
    (OUT / 'ocr' / (name + '.json')).write_text(
        json.dumps(lines, ensure_ascii=False, indent=1), encoding='utf-8')
    return lines


def main():
    global PID, WINDOW
    process = subprocess.Popen(
        [str(BINARY), '--config-dir', str(CONFIG)], env=env_for(CONFIG),
        cwd=str(RUN), stdout=open(OUT / 'tooltip-gpui.log', 'wb'),
        stderr=subprocess.STDOUT)
    PID = process.pid
    WINDOW = None
    try:
        deadline = time.time() + 40
        while time.time() < deadline:
            try:
                windows = json.loads(ax('windows', PID).stdout)
            except json.JSONDecodeError:
                windows = []
            for window in windows:
                if window.get('layer') == 0:
                    WINDOW = int(window['id'])
            if WINDOW:
                break
            time.sleep(0.3)
        if not WINDOW:
            raise SystemExit('no window')
        ax('front', PID)
        time.sleep(1.0)
        resize = ax('resize', PID, '1920', '1050').stdout
        log(resize.strip())
        time.sleep(1.5)
        ax('press', PID, '用量明细')
        time.sleep(2.0)
        for _ in range(8):
            ax('scroll', 960, 600, '1200')
            time.sleep(0.1)
        for _ in range(2):
            ax('scroll', 960, 600, '-600')
            time.sleep(0.6)
        dump = AXS / 'tooltip-probe.txt'
        ax('dump', PID, dump)
        rows = rows_of(dump)
        log('rows=%s' % rows)
        if not rows:
            raise SystemExit('no rows')
        first = rows[0]
        center = first['y'] + first['h'] // 2
        # Token column centre (both builds keep it around x=1000 after resize).
        for name, x in (('tooltip-tokens', 1000), ('tooltip-cost', 1330),
                        ('tooltip-credit', 1500)):
            ax('move', x, center)
            time.sleep(1.6)
            snapshot(name)
            ax('dump', PID, AXS / (name + '.txt'))
            ax('move', 5, 5)
            time.sleep(0.5)
            log('captured %s at x=%s y=%s' % (name, x, center))
        snapshot('tooltip-after')
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
        log('done rc=%s' % process.returncode)


if __name__ == '__main__':
    main()
