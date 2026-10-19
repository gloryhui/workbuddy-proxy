#!/usr/bin/env python3
"""Real-macOS GPUI acceptance drive for UI-T07-L01-THREE-SIZES-001.

One GPUI process is resized to the three requested window sizes
(1100x700, 1280x800, 1440x900).  At each size the driver scrolls the statistics
page to the request log, then records:

  01-left       default horizontal position (time/provider/model/request + left
                half of the token cell)
  02-token      horizontally scrolled: the full two-row token cell plus cost and
                credits
  03-status     horizontally scrolled to the right end: latency, tok/s, status
  04-rows       an attempt to scroll the log's own vertical viewport
  05-page2      next page through the real pager button (1/3 -> 2/3)

At 1440x900 one extra screen switches the source filter to built-in models so a
non-empty credit value (1.14) is visible in the same table.

Every screenshot is paired with a Vision OCR JSON, an AX tree dump and the
window geometry (AX frame, CGWindow bounds, PNG pixel size).  The driver never
touches a real profile, database, key or upstream: the core and the GPUI were
started by fixture.py against this task's throw-away config directory.

usage: gui_drive.py           (fixture.py setup must already have run)
"""
import json
import os
import re
import signal
import subprocess
import time
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l01-three-sizes-001'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/'
           '01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l01-three-sizes-001')
RUN = CAP / 'run'
CONFIG = RUN / 'config'
GPUI = ROOT / 'gpui/target/release/workbuddy-proxy-gpui'
AX = CAP / 'bin/axdrive'
HSCROLL = CAP / 'bin/hscroll'
OCR = CAP / 'bin/ocr'
LOG = EV / 'logs/gui-steps.log'
SHOTS = EV / 'shots'
AXDIR = EV / 'ax'
OCRDIR = EV / 'ocr'

SIZES = [(1100, 700), (1280, 800), (1440, 900)]
TOP_BAR = 30  # self-drawn title bar inside the window's content view


def log(message):
    line = '%s %s' % (time.strftime('%H:%M:%S'), message)
    with open(LOG, 'a', encoding='utf-8') as handle:
        handle.write(line + '\n')
    print(line, flush=True)


def call(*args, timeout=120):
    return subprocess.run([str(a) for a in args], capture_output=True,
                          text=True, timeout=timeout)


def ax(*args):
    result = call(AX, *args)
    if result.returncode != 0:
        log('axdrive %s rc=%s %s' % (args, result.returncode,
                                     result.stderr.strip()[:200]))
    return result


def env_for(config):
    env = dict(os.environ)
    env.update({
        'HOME': str(RUN / 'home'), 'TMPDIR': str(CAP / 'tmp'),
        'TMP': str(CAP / 'tmp'), 'TEMP': str(CAP / 'tmp'),
        'WORKBUDDY_PROXY_CONFIG_DIR': str(config),
        'XDG_CONFIG_HOME': str(RUN / 'home/.config'),
        'XDG_DATA_HOME': str(RUN / 'home/.local/share'),
        'XDG_CACHE_HOME': str(RUN / 'home/.cache'),
    })
    return env


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


def windows_json(pid):
    result = ax('windows', pid)
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return []


def dump(name):
    path = AXDIR / (name + '.txt')
    ax('dump', PID, path)
    return path


def png_size(path):
    data = path.read_bytes()[:33]
    return (int.from_bytes(data[16:20], 'big'), int.from_bytes(data[20:24], 'big'))


def ocr(path, out):
    result = call(OCR, path, 'zh-Hans,en-US')
    try:
        lines = json.loads(result.stdout)
    except json.JSONDecodeError:
        log('ocr error %s %s' % (result.stdout[:100], result.stderr[:100]))
        lines = []
    out.write_text(json.dumps(lines, ensure_ascii=False, indent=1), encoding='utf-8')
    return lines


def texts(lines):
    return [(line['text'], line['x'], line['y'], line['w'], line['h'])
            for line in lines if line.get('confidence', 0) >= 0.2]


def shot(name):
    png = SHOTS / (name + '.png')
    ax('shot', WINDOW, png)
    lines = ocr(png, OCRDIR / (name + '.json'))
    ax_path = dump(name)
    page = next((text for text, *_ in texts(lines) if re.search(r'第\s*\d', text)), '')
    times = sorted({text for text, *_ in texts(lines)
                    if re.match(r'10-19 \d\d:\d\d:\d\d', text)})
    record = {'step': name, 'image': str(png.relative_to(EV)),
              'ocr': str((OCRDIR / (name + '.json')).relative_to(EV)),
              'ax_dump': str(ax_path.relative_to(EV)),
              'png_size': list(png_size(png)), 'page_text': page,
              'visible_times': times,
              'ax_rows': ax_rows(ax_path)}
    log('shot %s png=%s page=%r times=%s' % (name, record['png_size'], page, times[:4]))
    return record


def ax_rows(path):
    rows = []
    for line in path.read_text(encoding='utf-8', errors='replace').splitlines():
        if 'AXRow' not in line:
            continue
        match = re.search(r'\[(-?\d+),(-?\d+) (\d+)x(\d+)\]', line)
        if match:
            rows.append([int(match.group(1)), int(match.group(2)),
                         int(match.group(3)), int(match.group(4))])
    return rows


def find_button(path, title):
    for line in path.read_text(encoding='utf-8', errors='replace').splitlines():
        if 'AXButton title=%s ' % title in line:
            match = re.search(r'\[(-?\d+),(-?\d+) (\d+)x(\d+)\]', line)
            if match:
                return [int(match.group(1)), int(match.group(2)),
                        int(match.group(3)), int(match.group(4))]
    return None


def find_radio(path, title):
    for line in path.read_text(encoding='utf-8', errors='replace').splitlines():
        if 'AXRadioButton title=%s ' % title in line:
            match = re.search(r'\[(-?\d+),(-?\d+) (\d+)x(\d+)\]', line)
            if match:
                return [int(match.group(1)), int(match.group(2)),
                        int(match.group(3)), int(match.group(4))]
    return None


def ensure_front():
    """The window can lose key status while the driver runs; every input event
    is preceded by an explicit activation so clicks/wheels reach the app."""
    ax('front', PID)
    time.sleep(0.15)


def click_frame(frame):
    ensure_front()
    x = frame[0] + frame[2] / 2
    y = frame[1] + frame[3] / 2
    ax('click', int(x), int(y))
    log('click %.0f,%.0f' % (x, y))


def page_scroll(dy, x=280, y=300):
    """Scroll the statistics page; the point stays left of the table so a
    wheel event never lands inside the table's own scroll area."""
    ensure_front()
    ax('scroll', int(x), int(y), dy)
    time.sleep(0.4)


def goto_table():
    """Scroll to the page bottom, then pull back just enough that the log card
    header and pager are fully on screen.  The wheel point stays outside the
    table so the table's own row offset is not changed here."""
    for _ in range(5):
        page_scroll(1800)
    last = None
    for _ in range(14):
        pager = find_button(dump('goto-table'), '下一页')
        y = pager[1] if pager else None
        if pager is not None and last is not None and abs(y - last) <= 2:
            break
        last = y
        page_scroll(-600)
    page_scroll(150)
    pager = find_button(dump('goto-table-settled'), '下一页')
    log('table settled pager=%s' % pager)
    return pager


def table_scroll_top(pager):
    """Put the log's own vertical scroll back at the first row."""
    ensure_front()
    for _ in range(4):
        ax('scroll', int(WIDTH / 2), int(pager[1] + 170), 1600)
        time.sleep(0.3)


def settle_table():
    """(Re)establish: page at the log card bottom, table scrolled to row 1."""
    pager = goto_table()
    table_scroll_top(pager)
    return goto_table()


def hscroll(dx, pager):
    """Horizontal wheel over a table row so only the table's own horizontal
    offset moves (the page itself has no horizontal axis)."""
    ensure_front()
    result = call(HSCROLL, int(WIDTH / 2), int(pager[1] + 170), dx)
    if result.returncode != 0:
        log('hscroll rc=%s %s' % (result.returncode, result.stderr.strip()[:200]))
    time.sleep(0.6)


def hscroll_to_left(pager):
    for _ in range(4):
        hscroll(1200, pager)


def row_scroll(dy, pager):
    """Scroll inside the log's row viewport, below its header and pager."""
    ensure_front()
    ax('scroll', int(WIDTH / 2), int(pager[1] + 170), dy)
    time.sleep(0.8)


def wait_page(expected, timeout=15):
    pattern = re.compile(r'第\s*%d\s*/\s*\d' % expected)
    deadline = time.time() + timeout
    last = ''
    while time.time() < deadline:
        probe = CAP / 'tmp' / 'page-probe.png'
        ax('shot', WINDOW, probe)
        lines = ocr(probe, CAP / 'tmp' / 'page-probe.json')
        last = next((text for text, *_ in texts(lines) if re.search(r'第\s*\d', text)), '')
        if pattern.search(last):
            return True
        time.sleep(0.8)
    log('wait_page timeout expected=%d last=%r' % (expected, last))
    return False


def settle(seconds=0.8):
    time.sleep(seconds)


def stop_gpui(process):
    if process is None:
        return
    try:
        process.terminate()
        process.wait(timeout=8)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)
    except ProcessLookupError:
        pass
    state_file = RUN / 'run.json'
    if state_file.exists():
        state = json.loads(state_file.read_text(encoding='utf-8'))
        state['gpui_pid'] = None
        state_file.write_text(json.dumps(state, indent=2), encoding='utf-8')


def main():
    for directory in (SHOTS, AXDIR, OCRDIR, CAP / 'tmp'):
        directory.mkdir(parents=True, exist_ok=True)
    LOG.write_text('', encoding='utf-8')
    log('starting GUI %s config=%s' % (GPUI, CONFIG))
    process = subprocess.Popen(
        [str(GPUI), '--config-dir', str(CONFIG)], env=env_for(CONFIG),
        cwd=str(RUN), stdout=open(EV / 'logs/gpui.stdout.log', 'wb'),
        stderr=open(EV / 'logs/gpui.stderr.log', 'wb'))
    global PID, WINDOW, WIDTH, HEIGHT
    PID = process.pid
    WINDOW = None
    try:
        state_file = RUN / 'run.json'
        if state_file.exists():
            state = json.loads(state_file.read_text(encoding='utf-8'))
            state['gpui_pid'] = PID
            state_file.write_text(json.dumps(state, indent=2), encoding='utf-8')
        deadline = time.time() + 40
        while time.time() < deadline:
            WINDOW = window_id(PID)
            if WINDOW:
                break
            if process.poll() is not None:
                raise RuntimeError('gpui exited early rc=%s' % process.returncode)
            time.sleep(0.3)
        if WINDOW is None:
            raise RuntimeError('gpui window did not appear')
        log('pid=%s window=%s' % (PID, WINDOW))
        ax('front', PID)
        settle(1.0)
        summary = {'pid': PID, 'window_id': WINDOW,
                   'binary': str(GPUI), 'title_bar_px': TOP_BAR,
                   'sizes': [], 'builtin_1440': None}
        for width, height in SIZES:
            WIDTH, HEIGHT = width, height
            step = {'requested': [width, height], 'screens': []}
            resize_out = ax('resize', PID, width, height).stdout.strip()
            log('resize %s -> %s' % (step['requested'], resize_out))
            settle(1.5)
            step['resize_output'] = resize_out
            step['windows'] = windows_json(PID)
            geometry = dump('window-%dx%d' % (width, height))
            step['ax_window_geometry'] = geometry.name
            initial = shot('size-%dx%d-00-initial' % (width, height))
            step['screens'].append(initial)
            step['window_frame_after_resize'] = initial['png_size']
            pager = settle_table()
            step['pager_frame'] = pager
            hscroll_to_left(pager)
            settle(0.5)
            step['screens'].append(shot('size-%dx%d-01-left' % (width, height)))
            hscroll(-340, pager)
            step['screens'].append(shot('size-%dx%d-02-token-cost' % (width, height)))
            hscroll(-900, pager)
            step['screens'].append(shot('size-%dx%d-03-status' % (width, height)))
            hscroll_to_left(pager)
            row_scroll(-500, pager)
            step['screens'].append(shot('size-%dx%d-04-rows-scrolled' % (width, height)))
            pager = settle_table()
            next_frame = find_button(dump('pre-next'), '下一页')
            if next_frame is None:
                raise RuntimeError('next-page button missing')
            click_frame(next_frame)
            step['page2_settled'] = wait_page(2)
            settle(0.6)
            step['screens'].append(shot('size-%dx%d-05-page2' % (width, height)))
            prev_frame = find_button(dump('pre-prev'), '上一页')
            if prev_frame is not None:
                click_frame(prev_frame)
                step['page1_settled'] = wait_page(1)
            settle(0.6)
            pager = settle_table()
            step['screens'].append(shot('size-%dx%d-06-back-page1' % (width, height)))
            summary['sizes'].append(step)
        # 1440x900: built-in source filter to show a non-empty credit value.
        WIDTH, HEIGHT = 1440, 900
        for _ in range(5):
            page_scroll(1800)
        settle(0.6)
        radio = find_radio(dump('pre-builtin'), '内置模型')
        if radio is None:
            summary['builtin_1440'] = {'opened': False, 'reason': 'radio missing'}
        else:
            click_frame(radio)
            settle(1.5)
            pager = settle_table()
            hscroll_to_left(pager)
            settle(0.5)
            builtin = shot('size-1440x900-07-builtin')
            summary['builtin_1440'] = {'opened': True, 'screen': builtin}
            for _ in range(5):
                page_scroll(1800)
            settle(0.6)
            radio = find_radio(dump('pre-all-sources'), '全部来源')
            if radio is not None:
                click_frame(radio)
                settle(1.5)
                pager = settle_table()
                summary['builtin_1440']['restored'] = True
                summary['builtin_1440']['restored_screen'] = shot(
                    'size-1440x900-08-all-sources-restored')
        (EV / 'gui-steps.json').write_text(
            json.dumps(summary, ensure_ascii=False, indent=2), encoding='utf-8')
    finally:
        stop_gpui(process)
        log('gui stopped')


if __name__ == '__main__':
    main()
