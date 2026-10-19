#!/usr/bin/env python3
"""Real GPUI pagination acceptance driver (UI-T07-L03-PAGESIZE-001).

Each step uses an AXPress on the visible pager button, then records:
  * a window-only PNG of the 1440x901 GPUI window,
  * Vision OCR of that PNG,
  * the full AX tree (with positions) of the application.

The page text and row ids are read from the PNG's OCR layer, because GPUI's
accesskit adapter does not expose the pager label or the disabled state as AX
attributes (all pager buttons report enabled=true). The disabled look is kept
in the screenshots and verified visually.
"""
import json
import re
import subprocess
import sys
import time
from pathlib import Path

EV = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign/docs/UI-UX-renew/evidence/t07-l03-pagesize-001')
TMP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp')
DRIVE = TMP / 'axdrive'
WINUTIL = TMP / 'winutil'
OCR = TMP / 'ocr'
PID = int(sys.argv[1]) if len(sys.argv) > 1 else 47673
WINDOW = int(sys.argv[2]) if len(sys.argv) > 2 else 7805
STEPS = [
    ('00-page1-20', None),
    ('01-page2-20', '下一页'),
    ('02-page3-20', '下一页'),
    ('03-page1-50', '20 条/页'),
    ('04-page2-50', '下一页'),
    ('05-page1-100', '50 条/页'),
]


def run(*args, stdout=None):
    if stdout is not None:
        return subprocess.run([str(a) for a in args], stdout=stdout,
                              stderr=subprocess.PIPE, text=True, timeout=90)
    return subprocess.run([str(a) for a in args], capture_output=True,
                          text=True, timeout=90)


def pager_y(dump):
    for line in dump.read_text(encoding='utf-8').splitlines():
        if 'AXButton title=下一页 ' in line:
            match = re.search(r'\[(-?\d+),(-?\d+) ', line)
            if match:
                return int(match.group(2))
    return None


def parse_ocr(path):
    lines = json.loads(path.read_text(encoding='utf-8'))
    texts = [line['text'] for line in lines if line['confidence'] >= 0.15]
    page = ''
    for text in texts:
        if '第' in text and '/' in text:
            page = text
            break
    size = next((text for text in texts if '条/页' in text), '')
    ids = sorted({re.sub(r'[^t0-9\-]', '', text) for text in texts
                  if re.search(r't0?7?1?0?3-\d{4}', text)})
    times = sorted({text for text in texts if re.match(r'10-19\s*18:', text)})
    return {'page_text': page, 'page_size_text': size, 'row_ids': ids, 'times': times}


def ensure_pager_visible(name):
    dump = EV / 'logs' / ('ax-pre-' + name + '.txt')
    run(DRIVE, 'dump', PID, dump)
    y = pager_y(dump)
    if y is None or y > 950:
        run(DRIVE, 'front', PID)
        time.sleep(0.3)
        run(WINUTIL, 'scroll', 760, 500, -700)
        time.sleep(1.0)
        run(DRIVE, 'dump', PID, dump)
        y = pager_y(dump)
    return y


def main():
    summary = {'pid': PID, 'window': WINDOW, 'steps': []}
    for name, action in STEPS:
        record = {'step': name, 'action': action}
        if action:
            pressed = run(DRIVE, 'press', PID, action, 'AXButton')
            record['press'] = pressed.stdout.strip()
            time.sleep(1.2)
        y = ensure_pager_visible(name)
        record['pager_y'] = y
        shot = EV / 'shots' / (name + '.png')
        ocr = EV / 'ocr' / (name + '.json')
        dump = EV / 'logs' / ('ax-' + name + '.txt')
        image = run(WINUTIL, 'shot', WINDOW, shot)
        record['image_result'] = image.stdout.strip()
        with open(ocr, 'w') as handle:
            run(OCR, shot, stdout=handle)
        run(DRIVE, 'dump', PID, dump)
        record.update({'image': str(shot.relative_to(EV)),
                       'ocr': str(ocr.relative_to(EV)),
                       'ax_dump': str(dump.relative_to(EV))})
        record.update(parse_ocr(ocr))
        rows = [line for line in dump.read_text(encoding='utf-8').splitlines()
                if line.strip().startswith('AXRow ')]
        record['ax_row_count'] = len(rows)
        summary['steps'].append(record)
        print(json.dumps(record, ensure_ascii=False))
    (EV / 'gui-steps.json').write_text(
        json.dumps(summary, ensure_ascii=False, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
