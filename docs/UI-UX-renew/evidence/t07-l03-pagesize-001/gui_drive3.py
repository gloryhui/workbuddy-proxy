#!/usr/bin/env python3
"""Real GPUI pagination acceptance driver, including the Previous button.

Steps (each AXPress reaches the same click handler as a mouse click):
  1. 20/page: page 1 -> 2 -> 3 (Next twice)
  2. Previous from page 3 -> page 2, proving the back path is not stale
  3. Next back to page 3
  4. 20 -> 50/page: must reset to page 1 of 2
  5. Next to page 2 of 2
  6. 50 -> 100/page: must reset to page 1 of 1
"""
import json
import re
import subprocess
import sys
import time
from pathlib import Path

EV = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign/docs/UI-UX-renew/evidence/t07-l03-pagesize-001')
TMP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp')
GUI = TMP / 'gui'
OCR = TMP / 'ocr'
PID = int(sys.argv[1])
WINDOW = int(sys.argv[2])
STEPS = [
    ('00-page1-20', None),
    ('01-page2-20', '下一页'),
    ('02-page3-20', '下一页'),
    ('03-prev-page2-20', '上一页'),
    ('04-page3-20-again', '下一页'),
    ('05-page1-50', '20 条/页'),
    ('06-page2-50', '下一页'),
    ('07-page1-100', '50 条/页'),
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
    page = next((text for text in texts if '第' in text and '/' in text), '')
    size = next((text for text in texts if '条/页' in text), '')
    ids = sorted({re.sub(r'[^0-9]', '', text)[-4:] for text in texts
                  if re.search(r'0?7?1?0?3-\d{4}', text)})
    times = sorted({text for text in texts if text.startswith('10-19')})
    return {'page_text': page, 'page_size_text': size,
            'row_suffixes': ids, 'times': times}


def ensure_pager_visible(name):
    dump = EV / 'logs' / ('ax-pre-' + name + '.txt')
    run(GUI, 'dump', PID, dump)
    y = pager_y(dump)
    if y is None or y > 950:
        run(GUI, 'front', PID)
        time.sleep(0.3)
        run(GUI, 'scroll', PID, 760, 500, -700)
        time.sleep(1.0)
        run(GUI, 'dump', PID, dump)
        y = pager_y(dump)
    return y


def main():
    summary = {'pid': PID, 'window': WINDOW, 'steps': []}
    run(GUI, 'front', PID)
    time.sleep(0.5)
    for name, action in STEPS:
        record = {'step': name, 'action': action}
        if action:
            pressed = run(GUI, 'press', PID, action)
            record['press'] = pressed.stdout.strip()
            time.sleep(1.2)
        record['pager_y'] = ensure_pager_visible(name)
        shot = EV / 'shots' / (name + '.png')
        ocr = EV / 'ocr' / (name + '.json')
        dump = EV / 'logs' / ('ax-' + name + '.txt')
        record['image_result'] = run(GUI, 'shot', WINDOW, shot).stdout.strip()
        with open(ocr, 'w') as handle:
            run(OCR, shot, stdout=handle)
        run(GUI, 'dump', PID, dump)
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
