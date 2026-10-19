#!/usr/bin/env python3
"""Real-GPUI acceptance driver for UI-T07-L04-GROUP-PAGING-SORT-001.

Runs one fixed sequence of single AXPress actions on the isolated GPUI window:
provider grouping -> paging -> page size -> sort -> model grouping -> the same
walk -> cross-tab cursor isolation -> request-log isolation. Every action is
performed once; after each action the driver waits until the OCR-derived state
matches the expected state twice in a row, then stores the window PNG, the OCR
JSON, the AX tree and the parsed state.

usage: gui_drive.py PID WINDOW_ID
"""
import json
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l04-group-paging-sort-001'
TMPBASE = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/'
               '01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l04-group-paging-sort-001')
AX = TMPBASE / 'bin/axdrive'
OCR = TMPBASE / 'bin/ocr'
SCRATCH = TMPBASE / 'scratch.png'
PID = int(sys.argv[1])
WINDOW = int(sys.argv[2])
SAMPLES = json.loads((EV / 'core-api-samples.json').read_text(encoding='utf-8'))


def run(*args, stdout=None):
    if stdout is not None:
        with open(stdout, 'w') as handle:
            return subprocess.run([str(a) for a in args], stdout=handle,
                                  stderr=subprocess.PIPE, text=True, timeout=120)
    return subprocess.run([str(a) for a in args], capture_output=True,
                          text=True, timeout=120)


def normalize(text):
    value = text.replace(' ', '')
    value = re.sub(r'(?i)mode[l1]', 'model', value)
    value = re.sub(r'(?i)prov[il]der', 'Provider', value)
    return value


def parse(lines):
    texts = [(line['text'], line['x'], line['y'], line['confidence'])
             for line in lines if line['confidence'] >= 0.25]
    out = {}
    for text, x, y, _ in texts:
        match = re.search(r'(\d+)\s*个分组', text.replace(' ', ''))
        if match:
            out['groups_total'] = int(match.group(1))
            break
    for text, x, y, _ in texts:
        match = re.search(r'(\d+)\s*条/页', text)
        if match:
            out['page_size'] = int(match.group(1))
            break
    for text, x, y, _ in texts:
        match = re.search(r'第\s*(\d+)\s*/\s*(\d+)', text)
        if match:
            out['rows_page'] = '%s/%s' % (match.group(1), match.group(2))
            break
    header = None
    for text, x, y, _ in texts:
        value = normalize(text)
        if y > 370 and x < 470:
            if value == '供应商':
                header = '供应商'
            elif value == '模型':
                header = '模型'
    if header:
        out['header'] = header
    candidates = []
    for text, x, y, _ in texts:
        if y < 400:
            continue
        value = normalize(text)
        match = re.search(r'Provider(\d{1,2})', value)
        if match:
            candidates.append((y, x, 'Provider %02d' % int(match.group(1))))
            continue
        match = re.search(r'model-?(\d{1,2})', value)
        if match:
            candidates.append((y, x, 'model-%02d' % int(match.group(1))))
    if candidates:
        candidates.sort()
        out['first_row'] = candidates[0][2]
        out['visible_labels'] = [item[2] for item in candidates]
    return out


def snapshot(path):
    run(AX, 'shot', WINDOW, path)
    result = subprocess.run([str(OCR), str(path)], capture_output=True,
                            text=True, timeout=120)
    try:
        return parse(json.loads(result.stdout))
    except json.JSONDecodeError:
        return {'ocr_error': result.stdout[:200] + result.stderr[:200]}


def matches(state, expect):
    return all(state.get(key) == value for key, value in expect.items())


def wait_for(expect, timeout=15.0):
    deadline = time.time() + timeout
    last = {}
    while time.time() < deadline:
        last = snapshot(SCRATCH)
        if matches(last, expect):
            time.sleep(0.7)
            confirm = snapshot(SCRATCH)
            if matches(confirm, expect):
                return confirm, True
            last = confirm
        time.sleep(0.4)
    return last, False


def wait_frame(timeout=10.0):
    """Wait until two consecutive OCR passes agree on the visible state."""
    deadline = time.time() + timeout
    previous = None
    while time.time() < deadline:
        current = snapshot(SCRATCH)
        if previous is not None and current == previous:
            return current
        previous = current
        time.sleep(0.7)
    return previous or {}


def first_label(group_by, sort, page_index):
    pages = SAMPLES['groups'][group_by]['walk_20'][sort]
    key = pages[page_index]['keys'][0]
    if group_by == 'provider':
        return 'Provider %02d' % int(key.split('-')[1])
    return key


def expect_group(group_by, sort, page_index):
    label = first_label(group_by, sort, page_index)
    return {
        'header': '供应商' if group_by == 'provider' else '模型',
        'first_row': label,
        'groups_total': 25 if group_by == 'provider' else 27,
        'page_size': 20,
    }


def step(summary, name, action=None, expect=None, note=''):
    record = {'step': name, 'action': action, 'note': note}
    if action:
        pressed = run(AX, 'press', PID, action)
        record['press'] = pressed.stdout.strip()
        record['press_rc'] = pressed.returncode
        if pressed.returncode != 0:
            record['press_stderr'] = pressed.stderr.strip()
    state, settled = wait_for(expect or {})
    record['settled'] = settled
    record['state'] = state
    record['expect'] = expect or {}
    shot = EV / 'shots' / (name + '.png')
    ocr = EV / 'ocr' / (name + '.json')
    dump = EV / 'logs' / ('ax-' + name + '.txt')
    run(AX, 'shot', WINDOW, shot)
    subprocess.run([str(OCR), str(shot)], stdout=open(ocr, 'w'),
                   stderr=subprocess.PIPE, text=True, timeout=120)
    run(AX, 'dump', PID, dump)
    record['image'] = str(shot.relative_to(EV))
    record['ocr'] = str(ocr.relative_to(EV))
    record['ax_dump'] = str(dump.relative_to(EV))
    record['passed'] = settled and matches(state, expect or {})
    summary['steps'].append(record)
    print(json.dumps({'step': name, 'action': action, 'passed': record['passed'],
                      'settled': settled, 'state': state,
                      'expect': expect}, ensure_ascii=False))
    return record


def main():
    summary = {'pid': PID, 'window': WINDOW, 'steps': []}
    # Make sure the window is frontmost and the detail card is in view.
    run(AX, 'front', PID)
    time.sleep(0.6)
    run(AX, 'click', 960, 90)
    time.sleep(0.8)
    dump = EV / 'logs' / 'ax-preflight.txt'
    run(AX, 'dump', PID, dump)
    text = dump.read_text(encoding='utf-8', errors='replace')
    if 'title=下一页' not in text or 'title=20 条/页' not in text:
        run(AX, 'scroll', 760, 500, -700)
        time.sleep(1.2)
        run(AX, 'dump', PID, dump)
        text = dump.read_text(encoding='utf-8', errors='replace')
    if 'title=下一页' not in text or 'title=20 条/页' not in text:
        # The detail card is below the fold; scroll once more before giving up.
        run(AX, 'scroll', 760, 500, -400)
        time.sleep(1.2)
        run(AX, 'dump', PID, dump)
        text = dump.read_text(encoding='utf-8', errors='replace')
    summary['preflight'] = {
        'pager_visible': 'title=下一页' in text and 'title=20 条/页' in text,
        'next_button': [line.strip() for line in text.splitlines()
                        if 'title=下一页' in line or 'title=上一页' in line],
    }

    step(summary, '01-requests-p1', '用量明细',
         {'rows_page': '1/3', 'page_size': 20},
         'request log baseline: 60 rows at 20/page is 3 pages')
    step(summary, '02-providers-p1-tokens', '供应商统计',
         expect_group('provider', 'tokens', 0), 'provider grouping, default sort=tokens')
    step(summary, '03-providers-p2-tokens', '下一页',
         expect_group('provider', 'tokens', 1), 'provider page 2 (5 groups)')
    record = step(summary, '04-providers-p2-next-boundary', '下一页',
                  expect_group('provider', 'tokens', 1), 'Next on the last page')
    boundary = run(AX, 'info', PID, '下一页')
    record['next_button_info'] = boundary.stdout.strip()
    step(summary, '05-providers-back-p1', '上一页',
         expect_group('provider', 'tokens', 0), 'Previous returns to page 1')
    step(summary, '06-providers-sort-requests-p1', '请求数',
         expect_group('provider', 'requests', 0), 'sort=requests re-queries providers')
    step(summary, '07-providers-p2-requests', '下一页',
         expect_group('provider', 'requests', 1), 'provider requests page 2')
    step(summary, '08-providers-sort-tokens-reset-p1', '总 Token',
         expect_group('provider', 'tokens', 0), 'sort=tokens resets to page 1')
    step(summary, '09-providers-size-50', '20 条/页',
         {'header': '供应商', 'first_row': 'Provider 07', 'groups_total': 25,
          'page_size': 50}, '50/page keeps 25 groups on one page')
    step(summary, '10-providers-size-100', '50 条/页',
         {'header': '供应商', 'first_row': 'Provider 07', 'groups_total': 25,
          'page_size': 100}, '100/page')
    step(summary, '11-providers-size-20', '100 条/页',
         expect_group('provider', 'tokens', 0), 'back to 20/page')
    step(summary, '12-providers-p2-before-tab', '下一页',
         expect_group('provider', 'tokens', 1), 'leave providers on page 2')
    step(summary, '13-models-p1-tokens', '模型统计',
         expect_group('model', 'tokens', 0),
         'model grouping starts on page 1 with sort=tokens; provider cursor not reused')
    step(summary, '14-models-p2-tokens', '下一页',
         expect_group('model', 'tokens', 1), 'model page 2 (7 groups)')
    step(summary, '15-models-back-p1', '上一页',
         expect_group('model', 'tokens', 0), 'model Previous')
    step(summary, '16-models-sort-requests-p1', '请求数',
         expect_group('model', 'requests', 0), 'model sort=requests')
    step(summary, '17-models-p2-requests', '下一页',
         expect_group('model', 'requests', 1), 'model requests page 2')
    step(summary, '18-models-sort-tokens-reset-p1', '总 Token',
         expect_group('model', 'tokens', 0), 'model sort=tokens resets page')
    step(summary, '19-models-p2-again', '下一页',
         expect_group('model', 'tokens', 1), 'leave models on page 2')
    step(summary, '20-providers-p1-after-models', '供应商统计',
         expect_group('provider', 'tokens', 0),
         'switching grouping resets to page 1 (cursor stacks are not shared)')
    step(summary, '21-requests-p1-isolation', '用量明细',
         {'rows_page': '1/3', 'page_size': 20},
         'request log still starts on page 1')
    step(summary, '22-requests-p2-isolation', '下一页',
         {'rows_page': '2/3', 'page_size': 20},
         'request log Next moves only the request log')
    step(summary, '23-providers-p1-after-rows-p2', '供应商统计',
         expect_group('provider', 'tokens', 0),
         'the request-log page 2 does not move the grouping cursor')

    settled = sum(1 for record in summary['steps'] if record['passed'])
    summary['passed'] = settled
    summary['total'] = len(summary['steps'])
    (EV / 'gui-steps.json').write_text(
        json.dumps(summary, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'passed': settled, 'total': len(summary['steps']),
                      'preflight': summary['preflight']}, ensure_ascii=False))


if __name__ == '__main__':
    main()
