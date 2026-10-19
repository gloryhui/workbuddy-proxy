#!/usr/bin/env python3
"""Fill the two captures the main current-binary run missed.

The current build's "built-in models" view has a single row; the time-text
clicked lookup did not find it, so this script clicks the first AX row slot
directly. It also hovers the token column by geometry instead of by OCR label
(the current two-line token cell can confuse the label search).

usage: supplement.py           (always drives the current binary)
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
BINARY = CAP / 'bin/gpui-current'
OUT = EV / 'raw' / 'current'
SHOTS = OUT / 'shots'
AXS = OUT / 'ax'
CONFIG = Path(json.loads((RUN / 'run.json').read_text())['config'])
state = json.loads((RUN / 'run.json').read_text())


def log(message):
    with open(OUT / 'supplement.log', 'a', encoding='utf-8') as handle:
        handle.write('%s %s\n' % (time.strftime('%H:%M:%S'), message))
    print(message, flush=True)


def call(*args):
    return subprocess.run([str(a) for a in args], capture_output=True,
                          text=True, timeout=120)


def ax(*args):
    return call(AX, *args)


def ocr(path):
    result = call(OCR, path, 'zh-Hans,en-US')
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return []


def window_id(pid):
    try:
        windows = json.loads(ax('windows', pid).stdout)
    except json.JSONDecodeError:
        return None
    for window in windows:
        if window.get('layer') == 0:
            return int(window['id'])
    return None


def snapshot(name):
    png = SHOTS / (name + '.png')
    ax('shot', WINDOW, png)
    lines = ocr(png)
    (OUT / 'ocr' / (name + '.json')).write_text(
        json.dumps(lines, ensure_ascii=False, indent=1), encoding='utf-8')
    return lines


def dump_ax(name):
    ax('dump', PID, AXS / (name + '.txt'))


def rows_of(dump):
    rows = []
    text = Path(dump).read_text(encoding='utf-8', errors='replace')
    for line in text.splitlines():
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


def scroll(dy):
    for _ in range(8):
        ax('scroll', 960, 600, str(dy if dy > 0 else 1200))
        time.sleep(0.1)
    for _ in range(2):
        ax('scroll', 960, 600, '-600')
        time.sleep(0.6)


def main():
    global PID, WINDOW
    process = subprocess.Popen(
        [str(BINARY), '--config-dir', str(CONFIG)], env=env_for(CONFIG),
        cwd=str(RUN), stdout=open(OUT / 'supplement-gpui.log', 'wb'),
        stderr=subprocess.STDOUT)
    PID = process.pid
    WINDOW = None
    try:
        deadline = time.time() + 40
        while time.time() < deadline:
            WINDOW = window_id(PID)
            if WINDOW:
                break
            time.sleep(0.3)
        if not WINDOW:
            raise SystemExit('no window')
        ax('front', PID)
        time.sleep(1.0)
        log(ax('resize', PID, '1920', '1050').stdout.strip())
        time.sleep(1.5)
        ax('press', PID, '用量明细')
        time.sleep(2.0)
        scroll(0)
        # R11: built-in view, single row -> click the first row slot.
        ax('press', PID, '内置模型')
        time.sleep(2.0)
        scroll(0)
        dump_ax('probe-r11')
        rows = rows_of(AXS / 'probe-r11.txt')
        log('r11 rows=%s' % rows)
        if rows:
            slot = rows[0]
            ax('click', slot['x'] + slot['w'] // 2, slot['y'] + slot['h'] // 2)
            time.sleep(1.6)
            snapshot('detail-r11-import-builtin')
            dump_ax('detail-r11-import-builtin')
            opened = False
            text = (AXS / 'detail-r11-import-builtin.txt').read_text(
                encoding='utf-8', errors='replace')
            opened = '用量详情' in text or '请求标识' in text
            log('r11 detail opened=%s' % opened)
            ax('escape', PID)
            time.sleep(1.0)
        # Tooltip: all sources, first row, token column by geometry.
        ax('press', PID, '全部来源')
        time.sleep(1.8)
        scroll(0)
        lines = snapshot('70-tooltip-top')
        dump_ax('70-tooltip-top')
        rows = rows_of(AXS / '70-tooltip-top.txt')
        log('tooltip rows=%s' % rows)
        if rows:
            slot = rows[0]
            ax('move', 950, slot['y'] + slot['h'] // 2)
            time.sleep(1.7)
            snapshot('tooltip-tokens-current')
            dump_ax('tooltip-tokens-current')
            ax('move', 5, 5)
            log('token tooltip captured')
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
