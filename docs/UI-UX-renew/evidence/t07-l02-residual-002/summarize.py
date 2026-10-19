#!/usr/bin/env python3
"""Summarize the T07-L02 screenshots for the residual recheck.

Reads only the saved OCR JSON next to each PNG (Vision boxes in PNG pixels) and
reports, per screen: the token column's header/data x positions, the token
column width (cost header minus token header) and the header/data sync delta.
No image is modified and no product code is touched.
"""
import json
from pathlib import Path

EV = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign/'
          'docs/UI-UX-renew/evidence/t07-l02-residual-002')
SCREENS = {
    '1100x700': ['1100-scroll-left', '1100-scroll-right', '1100-drag-min-240',
                 '1100-drag-min-240-again', '1100-drag-restored-340'],
    '1280x800': ['1280-scroll-left', '1280-scroll-right', '1280-drag-min-240',
                 '1280-drag-min-240-again', '1280-drag-restored-340'],
    '1440x900': ['1440-scroll-left', '1440-scroll-right', '1440-drag-min-240',
                 '1440-drag-min-240-again', '1440-drag-restored-340'],
}
LIMITS = {'time': 110, 'provider': 96, 'model': 88, 'request': 80,
          'tokens': 240, 'cost': 96, 'latency': 56, 'speed': 48, 'status': 92}


def find(ocr, names, y_max=380):
    for box in ocr:
        if box.get('confidence', 0) > 0.2 and box['text'].strip() in names \
                and box['y'] < y_max:
            return box
    return None


def main():
    script_dir = Path(__file__).resolve().parent
    summary = {
        'task': 'UI-T07-L02-RESIDUAL-002',
        'expected_token_min_width': 240,
        'design_drag_floors': LIMITS,
        'sizes': {},
    }
    for tag, screens in SCREENS.items():
        rows = []
        for name in screens:
            ocr_path = EV / 'ocr' / (name + '.json')
            if not ocr_path.exists():
                rows.append({'screen': name, 'missing': True})
                continue
            ocr = json.loads(ocr_path.read_text(encoding='utf-8'))
            header = find(ocr, {'Token/缓存命中', 'Token／缓存命中'})
            data = find(ocr, {'总 Token 2.22亿', '总Token2.22亿'})
            cost = find(ocr, {'估算费用'})
            status = find(ocr, {'状态'}, y_max=380)
            time_header = find(ocr, {'时间'})
            same_header_row = (header and cost
                               and -6 <= cost['y'] - header['y'] <= 22)
            width = (cost['x'] - header['x']) if same_header_row else None
            rows.append({
                'screen': name,
                'token_header_x': header['x'] if header else None,
                'token_data_x': data['x'] if data else None,
                'cost_header_x': cost['x'] if cost else None,
                'status_header_x': status['x'] if status else None,
                'time_header_x': time_header['x'] if time_header else None,
                'token_width_from_headers': width,
                'header_data_sync_delta': (
                    data['x'] - header['x'] if header and data else None),
            })
        summary['sizes'][tag] = rows
    (EV / 'acceptance-summary.json').write_text(
        json.dumps(summary, ensure_ascii=False, indent=2), encoding='utf-8')
    for tag, rows in summary['sizes'].items():
        for row in rows:
            print(tag, row)


if __name__ == '__main__':
    main()
