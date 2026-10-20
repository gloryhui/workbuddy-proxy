#!/usr/bin/env python3
"""Corrected 10-cycle X05 probe. Verifies filter values with AX each cycle."""
import csv, hashlib, json, os, subprocess, sys, time
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t09-x05-repeat-001/corrected'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t09-x05-repeat-corrected-001')
RUN = CAP / 'run'
AX = CAP / 'bin/axdrive'
OCR = CAP / 'bin/ocr'
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

def front():
    ax('front', PID)
    time.sleep(0.3)

def scroll(x, y, dy):
    ax('scroll', x, y, dy)

def shot(name):
    ax('shot', WID, str(EV / 'shots' / name))

def dump(name):
    ax('dump', PID, str(EV / 'ax' / name))

def status_value(name):
    path = EV / 'ax' / name
    dump(name)
    for line in path.read_text().splitlines():
        if 'AXPopUpButton title=请求状态' in line:
            return line.split('value=')[1].split(' enabled')[0]
    return None

def expect_status(name, expected, cycle):
    got = status_value(name)
    if got != expected:
        failure = {'cycle': cycle, 'step': name, 'expected': expected, 'got': got}
        (EV / 'logs/corrected-failure.json').write_text(json.dumps(failure, ensure_ascii=False, indent=2))
        raise RuntimeError('filter status mismatch: %r' % failure)
    return got

def kpi(name):
    # KPI text is not exposed by AX. Capture the small KPI card, crop it with
    # sips, and OCR the crop; this keeps the per-cycle verification quick.
    img = EV / 'shots' / (name + '.png')
    crop = EV / 'shots' / (name + '-crop.png')
    shot(name + '.png')
    sh(['sips', '-c', '160', '220', '--cropOffset', '340', '580', str(img), '--out', str(crop)])
    result = sh([str(OCR), str(crop)], timeout=30)
    data = json.loads(result.stdout)
    text = ''.join(t['text'] for t in data if t['confidence'] > 0.2).replace(' ', '')
    (EV / 'ocr' / (name + '.json')).write_text(json.dumps(data, ensure_ascii=False, indent=2))
    return text

def ps_metrics(pid):
    out = sh(['ps', '-o', 'rss=,cputime=', '-p', str(pid)]).stdout.strip()
    if not out:
        return {'rss_kb': None, 'cpu_s': None}
    rss, cpu = out.split(None, 1)
    sec = 0.0
    for p in cpu.split(':'):
        sec = sec * 60 + float(p)
    return {'rss_kb': int(rss), 'cpu_s': round(sec, 2)}

def fd_count(pid):
    return max(0, len(sh(['lsof', '-p', str(pid)]).stdout.splitlines()) - 1)

def thread_count(pid):
    return max(0, len(sh(['ps', '-M', str(pid)]).stdout.splitlines()) - 1)

def window_count():
    try:
        return len(json.loads(ax('windows', PID)))
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
    front()
    click(127, 151); time.sleep(0.6)
    initial_status = status_value('00-initial.txt')
    if initial_status != '全部状态':
        raise RuntimeError('fresh fixture initial filter is %r, expected 全部状态' % initial_status)
    initial_text = kpi('00-initial')
    rows = [snapshot('initial', {'filter_status': initial_status, 'kpi_has_20': '20代理' in initial_text})]
    dump('00-initial.txt')
    for i in range(1, cycles + 1):
        stage = {}
        front()
        t0 = time.monotonic()
        click(127, 248); time.sleep(0.35); stage['provider_A_s'] = round(time.monotonic() - t0, 3)
        t = time.monotonic(); click(1051, 330); time.sleep(0.25); escape(); time.sleep(0.2)
        stage['protocol_s'] = round(time.monotonic() - t, 3)
        t = time.monotonic(); click(961, 462); time.sleep(0.8)
        picker_path = EV / 'ax' / ('cycle-%02d-picker.txt' % i)
        sh([str(AX), 'dump', str(PID), str(picker_path)])
        if 'mock-model-a' not in picker_path.read_text() and 'mock-model' not in picker_path.read_text():
            raise RuntimeError('picker did not show mock models on cycle %d' % i)
        escape(); time.sleep(0.2); stage['picker_s'] = round(time.monotonic() - t, 3)
        t = time.monotonic(); click(127, 284); time.sleep(0.25); click(127, 248); time.sleep(0.25)
        stage['provider_B_A_s'] = round(time.monotonic() - t, 3)
        t = time.monotonic(); click(127, 151); time.sleep(0.6)
        # The filter must start at 全部状态 (previous cycle restored it).
        pre = expect_status('cycle-%02d-filter-pre.txt' % i, '全部状态', i)
        click(698, 216); time.sleep(0.25); click(680, 295); time.sleep(0.9)
        failed = expect_status('cycle-%02d-filter-failed.txt' % i, '失败', i)
        failed_text = kpi('cycle-%02d-filter-failed' % i)
        if '0代理' not in failed_text:
            raise RuntimeError('cycle %d failed filter has no 0-agents KPI: %r' % (i, failed_text[:300]))
        click(698, 216); time.sleep(0.25); click(680, 250); time.sleep(0.9)
        restored = expect_status('cycle-%02d-filter-all.txt' % i, '全部状态', i)
        restored_text = kpi('cycle-%02d-filter-all' % i)
        if '20代理' not in restored_text:
            raise RuntimeError('cycle %d restored filter has no 20-agents KPI: %r' % (i, restored_text[:300]))
        stage['filter_s'] = round(time.monotonic() - t, 3)
        t = time.monotonic(); scroll(1400, 700, -700); time.sleep(0.2); scroll(1400, 700, 700); time.sleep(0.2)
        stage['scroll_s'] = round(time.monotonic() - t, 3)
        rec = snapshot('cycle-%02d' % i, {'filter_status': restored, 'failed_status': failed,
                                          'kpi_failed_0': '0代理' in failed_text, 'kpi_restored_20': '20代理' in restored_text, **stage})
        rec['cycle_s'] = round(time.monotonic() - t0, 3)
        rows.append(rec)
        if i in (1, 5, 10):
            shot('cycle-%02d-main.png' % i)
            dump('cycle-%02d-main.txt' % i)
    rows.append(snapshot('final', {'filter_status': status_value('99-final.txt')}))
    shot('99-final.png')
    fields = list(rows[0].keys())
    for r in rows:
        for k in r:
            if k not in fields:
                fields.append(k)
    with open(EV / 'logs/measurements-corrected.csv', 'w', newline='') as f:
        w = csv.DictWriter(f, fieldnames=fields); w.writeheader(); w.writerows(rows)
    with open(EV / 'logs/measurements-corrected.json', 'w') as f:
        json.dump(rows, f, ensure_ascii=False, indent=2)
    print(json.dumps({'initial': rows[0], 'final': rows[-1], 'cycles': cycles}, ensure_ascii=False, indent=2))

if __name__ == '__main__':
    main()
