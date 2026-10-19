#!/usr/bin/env python3
"""Real-GPUI capture for UI-T07-L05-FULL-VALUES-001.

Starts one GPUI binary against the isolated fixture core and drives one fixed
sequence on both binaries, storing every window screenshot and AX tree:

  1. usage log + "all sources" (the default proxy capture): six screens down
  2. "built-in models": the imported built-in row
  3. "custom models": the imported custom row
  4. usage detail dialogs for the representative rows
  5. hover tooltips over the token cell and the cost cell

The window is resized to fill the screen first, so all columns stay inside one
viewport on both binaries. Rows are identified later from the time column, not
from the request id (the request id is a low-contrast cell Vision misreads).

usage: capture.py BINARY LABEL        (LABEL = pre-t07 | current)
"""
import json
import os
import re
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
RAW = EV / 'raw'
WIDTH, HEIGHT = 1920, 1050
SCREENS = 6
SCROLL_STEP = -250

BINARY = Path(sys.argv[1])
LABEL = sys.argv[2]
WIN_X, WIN_Y = 0, 0
OUT = RAW / LABEL
SHOTS = OUT / 'shots'
AXS = OUT / 'ax'
LOG = OUT / 'raw.log'

state = json.loads((RUN / 'run.json').read_text(encoding='utf-8'))
CONFIG = Path(state['config'])
EXPECTED = json.loads((EV / 'expected.json').read_text(encoding='utf-8'))


def log(message):
    line = '%s %s' % (time.strftime('%H:%M:%S'), message)
    with open(LOG, 'a', encoding='utf-8') as handle:
        handle.write(line + '\n')
    print(line, flush=True)


def env_for(config):
    env = dict(os.environ)
    env.update({
        'HOME': str(RUN / 'home'),
        'TMPDIR': str(RUN / 'tmp'),
        'TMP': str(RUN / 'tmp'),
        'TEMP': str(RUN / 'tmp'),
        'WORKBUDDY_PROXY_CONFIG_DIR': str(config),
        'XDG_CONFIG_HOME': str(RUN / 'home/.config'),
        'XDG_DATA_HOME': str(RUN / 'home/.local/share'),
        'XDG_CACHE_HOME': str(RUN / 'home/.cache'),
    })
    return env


def call(*args, timeout=120):
    return subprocess.run([str(a) for a in args], capture_output=True,
                          text=True, timeout=timeout)


def ax(*args, timeout=120):
    result = call(AX, *args, timeout=timeout)
    if result.returncode != 0:
        log('axdrive %s rc=%s stderr=%s' % (args, result.returncode,
                                            result.stderr.strip()[:300]))
    return result


def ocr(path):
    result = call(OCR, path, 'zh-Hans,en-US')
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        log('ocr error %s %s' % (result.stdout[:120], result.stderr[:120]))
        return []


def window_id(pid):
    result = ax('windows', pid)
    try:
        windows = json.loads(result.stdout)
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
    (OUT / 'ocr').mkdir(parents=True, exist_ok=True)
    (OUT / 'ocr' / (name + '.json')).write_text(
        json.dumps(lines, ensure_ascii=False, indent=1), encoding='utf-8')
    return lines


def dump_ax(name):
    path = AXS / (name + '.txt')
    ax('dump', PID, path)
    return path


def texts(lines):
    return [(line['text'], line['x'], line['y'], line['w'], line['h'])
            for line in lines if line.get('confidence', 0) >= 0.2]


def find_text(lines, needle):
    for text, x, y, w, h in texts(lines):
        if needle in text:
            return (x, y, w, h)
    return None


def scroll(dy, x=960, y=600):
    ax('scroll', x, y, str(dy))


def scroll_to_top():
    for _ in range(8):
        scroll(1200)
        time.sleep(0.1)
    time.sleep(0.5)


def bring_table_into_view():
    scroll_to_top()
    scroll(-600)
    time.sleep(0.8)
    scroll(-600)
    time.sleep(1.0)


def press_title(title, label=None):
    """AXPress a titled control (works even when it is scrolled out of view)."""
    result = ax('press', PID, title)
    ok = 'pressed=1' in result.stdout
    if not ok:
        log('press_title MISS %s (%s) -> %s' % (title, label or '', result.stdout.strip()))
    else:
        log('press_title %s (%s)' % (title, label or ''))
    return ok


def click_text(lines, needle, label):
    box = find_text(lines, needle)
    if box is None:
        log('click_text MISS %s' % label)
        return False
    x, y, w, h = box
    ax('click', int(x + w / 2 + WIN_X), int(y + h / 2 + WIN_Y))
    log('click_text %s at %s,%s' % (label, int(x + w / 2 + WIN_X),
                                    int(y + h / 2 + WIN_Y)))
    return True


def wait_text(needle, timeout=20.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        lines = snapshot('probe')
        if find_text(lines, needle):
            return lines
        time.sleep(0.5)
    log('wait_text timeout %s' % needle)
    return lines


def capture_screens(phase):
    """Capture SCREENS screens of the current view, always starting at the top."""
    bring_table_into_view()
    screens = []
    for index in range(SCREENS):
        name = '%s-%02d' % (phase, index)
        snapshot(name)
        dump_ax(name)
        screens.append(name)
        log('captured %s' % name)
        if index + 1 < SCREENS:
            scroll(SCROLL_STEP)
            time.sleep(1.0)
    return screens


def parse_rows(path):
    """AXRow frames of one dump, ordered top to bottom."""
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


def compact(text):
    return re.sub(r'\s+', '', text)


def open_detail(time_text, name):
    """Click the row whose time cell shows `time_text`, then store the dialog."""
    bring_table_into_view()
    for attempt in range(7):
        lines = snapshot('detailprobe-')
        box = None
        needle = compact(time_text)
        for text, x, y, w, h in texts(lines):
            if needle in compact(text):
                box = (x, y, w, h)
                break
        if box is not None:
            x, y, w, h = box
            ax('click', int(x + w / 2 + WIN_X), int(y + h / 2 + WIN_Y))
            time.sleep(1.5)
            detail = snapshot('detail-' + name)
            dump_ax('detail-' + name)
            opened = (find_text(detail, '用量详情') is not None
                      or find_text(detail, '请求标识') is not None)
            record = {'name': name, 'time': time_text, 'opened': opened,
                      'clicked_at': [int(x + w / 2 + WIN_X), int(y + h / 2 + WIN_Y)],
                      'scroll_attempts': attempt}
            log('detail %s opened=%s attempt=%s' % (name, opened, attempt))
            return record
        scroll(-200)
        time.sleep(0.9)
    return {'name': name, 'time': time_text, 'opened': False}


def close_detail():
    ax('escape', PID)
    time.sleep(1.0)
    lines = snapshot('after-detail-close')
    if find_text(lines, '用量详情'):
        ax('click', 60, 1000)
        time.sleep(1.0)
        log('detail closed by overlay click')
        return 'overlay'
    return 'escape'


def hover(lines, needle, name):
    box = find_text(lines, needle)
    if box is None:
        log('hover MISS %s' % needle)
        return {'name': name, 'found': False}
    x, y, w, h = box
    ax('move', int(x + w / 2 + WIN_X), int(y + h / 2 + WIN_Y))
    time.sleep(1.6)
    snapshot(name)
    dump_ax(name)
    ax('move', 5, 5)
    time.sleep(0.5)
    return {'name': name, 'found': True,
            'anchor': [int(x + w / 2 + WIN_X), int(y + h / 2 + WIN_Y)]}


def row_times():
    return {row['id']: row['expected']['time'] for row in EXPECTED['rows']}


def kill_stale():
    """Terminate any GPUI still holding this task config's lock."""
    result = subprocess.run(['ps', '-eo', 'pid=,command='], capture_output=True,
                            text=True)
    stale = []
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line or 'gpui' not in line or str(CONFIG) not in line:
            continue
        pid = int(line.split()[0])
        if pid == os.getpid():
            continue
        stale.append(pid)
    for pid in stale:
        log('terminating stale gpui pid=%s' % pid)
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    for pid in stale:
        for _ in range(60):
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                break
            time.sleep(0.1)
        else:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
    if stale:
        time.sleep(0.5)


def main():
    for directory in (SHOTS, AXS, OUT / 'ocr'):
        directory.mkdir(parents=True, exist_ok=True)
    LOG.write_text('', encoding='utf-8')
    log('starting %s label=%s' % (BINARY, LABEL))
    kill_stale()
    process = subprocess.Popen(
        [str(BINARY), '--config-dir', str(CONFIG)], env=env_for(CONFIG),
        cwd=str(RUN),
        stdout=open(OUT / 'gpui.stdout.log', 'wb'),
        stderr=open(OUT / 'gpui.stderr.log', 'wb'))
    global PID, WINDOW
    PID = process.pid
    WINDOW = None
    deadline = time.time() + 40
    while time.time() < deadline:
        WINDOW = window_id(PID)
        if WINDOW:
            break
        if process.poll() is not None:
            raise SystemExit('gpui exited early rc=%s' % process.returncode)
        time.sleep(0.3)
    if WINDOW is None:
        process.kill()
        raise SystemExit('gpui window did not appear')
    log('pid=%s window=%s' % (PID, WINDOW))
    ax('front', PID)
    time.sleep(1.0)
    resize_out = ax('resize', PID, str(WIDTH), str(HEIGHT)).stdout.strip()
    log('resize ' + resize_out)
    time.sleep(1.5)
    global WIN_X, WIN_Y
    match = re.search(r'frame=\[(-?\d+),(-?\d+)', resize_out)
    if match:
        WIN_X, WIN_Y = int(match.group(1)), int(match.group(2))
    log('window offset = %s,%s' % (WIN_X, WIN_Y))
    summary = {'label': LABEL, 'binary': str(BINARY), 'pid': PID, 'window_offset': [WIN_X, WIN_Y],
               'window': WINDOW, 'requested_size': [WIDTH, HEIGHT],
               'views': {}, 'details': [], 'tooltips': [],
               'screens_per_view': SCREENS, 'scroll_step': SCROLL_STEP}
    try:
        lines = snapshot('00-initial')
        dump_ax('00-initial')
        windows = json.loads(ax('windows', PID).stdout or '[]')
        summary['windows'] = windows

        if not press_title('用量明细', 'usage-tab'):
            raise SystemExit('usage tab not found')
        time.sleep(1.5)
        bring_table_into_view()
        lines = wait_text('时间', timeout=10.0)
        time.sleep(0.8)

        summary['views']['all-sources'] = capture_screens('10-all-sources')

        press_title('内置模型', 'builtin')
        time.sleep(1.5)
        bring_table_into_view()
        lines = wait_text('WorkBuddy', timeout=10.0)
        time.sleep(0.8)
        summary['views']['builtin'] = capture_screens('20-builtin')

        press_title('自定义模型', 'custom')
        time.sleep(1.5)
        bring_table_into_view()
        lines = wait_text('时间', timeout=10.0)
        time.sleep(0.8)
        summary['views']['custom'] = capture_screens('30-custom')

        press_title('全部来源', 'all-sources')
        time.sleep(1.5)
        bring_table_into_view()
        lines = wait_text('时间', timeout=10.0)
        time.sleep(0.8)

        times = row_times()
        detail_targets = [
            ('r01-complete-default-zero', times[1]),
            ('r02-priced-streaming', times[2]),
            ('r05-zero-usage', times[5]),
            ('r06-partial-usage', times[6]),
            ('r07-failed-unpriced', times[7]),
            ('r09-in-flight', times[9]),
            ('r10-cancelled', times[10]),
            ('r16-legacy-long-names', times[16]),
        ]
        for name, time_text in detail_targets:
            summary['details'].append(open_detail(time_text, name))
            summary['details'][-1]['close'] = close_detail()

        press_title('内置模型', 'builtin-detail')
        time.sleep(1.5)
        summary['details'].append(open_detail(times[11], 'r11-import-builtin'))
        summary['details'][-1]['close'] = close_detail()

        press_title('自定义模型', 'custom-detail')
        time.sleep(1.5)
        summary['details'].append(open_detail(times[12], 'r12-import-custom'))
        summary['details'][-1]['close'] = close_detail()

        press_title('全部来源', 'all-tooltip')
        time.sleep(1.5)
        bring_table_into_view()
        lines = snapshot('70-tooltip-top')
        dump_ax('70-tooltip-top')
        summary['tooltips'].append(hover(lines, '总Token', 'tooltip-tokens-current'))
        lines = snapshot('71-after-tooltip')
        summary['tooltips'].append(hover(lines, '估算费用', 'tooltip-cost-header'))
        summary['tooltips'].append(hover(lines, '未定价', 'tooltip-unpriced'))

        summary['exit'] = 'terminating'
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
        summary['exit_code'] = process.returncode
        (OUT / 'capture.json').write_text(
            json.dumps(summary, ensure_ascii=False, indent=2), encoding='utf-8')
        summary['exit'] = 'completed'
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
        summary['exit_code'] = process.returncode
        (OUT / 'capture.json').write_text(
            json.dumps(summary, ensure_ascii=False, indent=2), encoding='utf-8')
        log('done rc=%s' % process.returncode)


if __name__ == '__main__':
    main()
