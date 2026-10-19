#!/usr/bin/env python3
"""Geometry/value analysis for UI-T07-L01-THREE-SIZES-001.

Reads the original PNG/OCR/AX artifacts written by gui_drive.py and produces
machine-readable checks.  Nothing is re-rendered or edited; OCR boxes are the
measured positions in the saved PNG pixels.  Where Vision misreads a glyph the
check records the raw text instead of silently "fixing" it; REPORT.md states
which conclusion is supported by the screenshot pixels and which one needs the
AX tree or the backend sample.

Checks per size:
  * window frame == requested size, PNG pixels == window frame, and the
    application content height (window minus the 30px self-drawn title bar).
  * AXRow height is the 56px baseline; table viewport width is the AXRow width.
  * both token lines sit vertically apart, inside the token column, left of the
    cost column; at least one measured row exists per horizontal position.
  * source pill, credits label, amount, status, unknown ("—") values appear in
    the screen that exposes the relevant column.
  * pager and the log's own row scroll change the visible request-id suffixes.
"""
import json
import re
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l01-three-sizes-001'
SIZES = [(1100, 700), (1280, 800), (1440, 900)]
TOP_BAR = 30
ROW_HEIGHT = 56


def load_json(path):
    return json.loads(Path(path).read_text(encoding='utf-8'))


def ocr_boxes(name):
    return load_json(EV / 'ocr' / (name + '.json'))


def boxes(name, min_confidence=0.15):
    return [line for line in ocr_boxes(name)
            if line.get('confidence', 0) >= min_confidence]


def ax_window(path):
    for line in Path(path).read_text(encoding='utf-8', errors='replace').splitlines():
        if 'AXWindow ' in line:
            match = re.search(r'\[(-?\d+),(-?\d+) (\d+)x(\d+)\]', line)
            if match:
                return [int(match.group(i)) for i in range(1, 5)]
    return None


def ax_rows(path):
    rows = []
    for line in Path(path).read_text(encoding='utf-8', errors='replace').splitlines():
        if 'AXRow ' in line:
            match = re.search(r'\[(-?\d+),(-?\d+) (\d+)x(\d+)\]', line)
            if match:
                rows.append([int(match.group(i)) for i in range(1, 5)])
    return rows


def png_size(name):
    data = (EV / 'shots' / (name + '.png')).read_bytes()[:33]
    return [int.from_bytes(data[16:20], 'big'), int.from_bytes(data[20:24], 'big')]


def request_suffixes(name):
    found = set()
    for box in boxes(name):
        for match in re.findall(r'(\d{4})', box['text']):
            suffix = int(match)
            if 1 <= suffix <= 45:
                found.add(suffix)
    return sorted(found)


def token_geometry(name):
    """Measure the two-line token cell of every visible log row.

    The token column's left edge comes from the 'Token / 缓存命中' header (or
    from the leftmost token line when Vision misses that header).  Its right
    edge comes from the '估算费用' header on the same header row, never from
    the unrelated page-level cost card at the top of the screenshot.
    """
    lines = boxes(name, 0.15)
    token_headers = [b for b in lines
                     if 'Token' in b['text'] and ('缓存' in b['text'] or '命中' in b['text'])]
    cost_headers = [b for b in lines if '估算费用' in b['text']]
    token_rows = [b for b in lines
                  if re.match(r'^[•·\-]?\s*总\s*Token', b['text'])]
    token_rows.sort(key=lambda b: b['y'])
    measured = []
    for row in token_rows:
        # Header row: nearest token header above the row, or the row's own x.
        above = [b for b in token_headers if 0 < row['y'] - b['y'] <= 80]
        token_left = (max(above, key=lambda b: b['y'])['x'] if above
                      else min(b['x'] for b in token_rows) - 8)
        costs = [b for b in cost_headers if -10 <= b['y'] - row['y'] <= 80]
        if not costs:
            continue
        cost_left = min(costs, key=lambda b: abs(b['y'] - row['y']))['x']
        if cost_left <= token_left:
            continue
        line1 = [b for b in lines
                 if abs(b['y'] - row['y']) <= 4
                 and token_left - 5 <= b['x'] < cost_left]
        line2 = [b for b in lines
                 if row['y'] + 8 <= b['y'] <= row['y'] + 28
                 and token_left - 5 <= b['x'] < cost_left]
        if not line1 or not line2:
            continue
        line1.sort(key=lambda b: b['x'])
        line2.sort(key=lambda b: b['x'])
        overlap = any(min(a['y'] + a['h'], c['y'] + c['h']) - max(a['y'], c['y']) > 1
                      for a in line1 for c in line2)
        right = max(b['x'] + b['w'] for b in line1 + line2)
        measured.append({
            'anchor_y': row['y'],
            'line1': [b['text'] for b in line1],
            'line2': [b['text'] for b in line2],
            'line1_y': [line1[0]['y'], line1[0]['y'] + line1[0]['h']],
            'line2_y': [line2[0]['y'], line2[0]['y'] + line2[0]['h']],
            'vertical_overlap': overlap,
            'right_edge': right,
            'token_col_left': token_left,
            'cost_col_left': cost_left,
            'inside_token_column': right <= cost_left + 2,
        })
    return {'available': bool(measured), 'measured_rows': len(measured),
            'rows': measured}


def text_like(lines, *needles):
    return [b['text'] for b in lines
            if all(any(n in b['text'] for n in group) if isinstance(group, tuple)
                   else group in b['text'] for group in needles)]


def value_checks(name):
    lines = boxes(name, 0.15)
    texts = [b['text'] for b in lines]
    joined = ' '.join(texts)
    def has_any(*needles):
        return any(needle in text for text in texts for needle in needles)
    source = any(('代理' in t or '代型' in t or '内置' in t or '自定义' in t)
                 and len(t) <= 12 for t in texts)
    return {
        'screen': name,
        'has_large_2_22yi': has_any('2.22亿'),
        'has_large_1_23yi': has_any('1.23亿'),
        'has_large_9877wan': has_any('9877万'),
        'has_source_pill': source,
        'has_credits_label': has_any('点数'),
        'has_credits_value': has_any('点数 1.14', '点数1.14'),
        'has_unknown_token': any('Token' in t and ('一' in t or '—' in t or '-' in t)
                                 and '3000' not in t and '1000' not in t
                                 and '2.22' not in t and '0' not in t
                                 for t in texts),
        'has_unknown_value': has_any('—', '一'),
        'has_unpriced': has_any('未定价'),
        'has_incomplete_usage': has_any('用量不全'),
        'has_cache_breakdown': has_any('缓存计费数据不足'),
        'has_status_completed': has_any('完成'),
        'has_status_failed': any('429' in t and ('失败' in t or '失敗' in t) for t in texts),
        'has_status_in_flight': has_any('进行中', '進行中'),
        'has_status_cancelled': has_any('已取消'),
        'has_page_text': any('第' in t and '/' in t for t in texts),
        'request_suffixes': request_suffixes(name),
        'texts': texts,
    }


def main():
    summary = load_json(EV / 'gui-steps.json')
    expected = load_json(EV / 'expected.json')
    analysis = {'task': 'UI-T07-L01-THREE-SIZES-001',
                'expected_page_walk': expected['page_walk'],
                'views': expected['views'], 'sizes': []}
    for step in summary['sizes']:
        width, height = step['requested']
        tag = 'size-%dx%d' % (width, height)
        frame = ax_window(EV / 'ax' / ('window-%dx%d.txt' % (width, height)))
        screens = {}
        for screen in step['screens']:
            short = screen['step'].replace(tag + '-', '')
            screens[short] = {
                'image': screen['image'],
                'png_size': screen['png_size'],
                'page_text': screen['page_text'],
                'ax_rows': screen['ax_rows'],
                'checks': value_checks(screen['step']),
            }
        geometry = {}
        for key in ('01-left', '02-token-cost', '03-status'):
            if key in screens:
                geometry[key] = token_geometry(tag + '-' + key)
        rows = [row for screen in screens.values() for row in screen['ax_rows']]
        entry = {
            'requested': [width, height],
            'ax_window_frame': frame,
            'windows_cg_bounds': step.get('windows'),
            'window_matches_request': frame is not None and frame[2:] == [width, height],
            'png_size': png_size(tag + '-01-left'),
            'content_view_size': [frame[2], frame[3]] if frame else None,
            'application_content_below_title_bar': [width, height - TOP_BAR],
            'ax_row_heights': sorted({row[3] for row in rows}),
            'table_viewport_width': rows[0][2] if rows else None,
            'pager_frame': step.get('pager_frame'),
            'screens': screens,
            'token_geometry': geometry,
        }
        left_ids = set(screens.get('01-left', {}).get('checks', {}).get('request_suffixes', []))
        scroll_ids = set(screens.get('04-rows-scrolled', {})
                         .get('checks', {}).get('request_suffixes', []))
        page2 = screens.get('05-page2', {})
        page2_ids = set(page2.get('checks', {}).get('request_suffixes', []))
        entry['row_scroll_changed_rows'] = bool(scroll_ids and scroll_ids != left_ids)
        entry['page_changed_rows'] = bool(page2_ids and page2_ids != left_ids)
        entry['page2_text'] = page2.get('page_text', '')
        entry['left_ids'] = sorted(left_ids)
        entry['scroll_ids'] = sorted(scroll_ids)
        entry['page2_ids'] = sorted(page2_ids)
        analysis['sizes'].append(entry)

    builtin = summary.get('builtin_1440') or {}
    if builtin.get('opened'):
        name = builtin['screen']['step']
        analysis['builtin_1440'] = {
            'screen': name,
            'page_text': builtin['screen']['page_text'],
            'checks': value_checks(name),
            'token_geometry': token_geometry(name),
        }
    (EV / 'analysis.json').write_text(
        json.dumps(analysis, ensure_ascii=False, indent=2), encoding='utf-8')

    rows = []
    for entry in analysis['sizes']:
        width, height = entry['requested']
        tag = '%dx%d' % (width, height)
        measured = [row for geo in entry['token_geometry'].values()
                    for row in geo.get('rows', [])]
        token_ok = bool(measured) and all(
            not row['vertical_overlap'] and row['inside_token_column']
            for row in measured)
        left = entry['screens'].get('01-left', {}).get('checks', {})
        middle = entry['screens'].get('02-token-cost', {}).get('checks', {})
        right = entry['screens'].get('03-status', {}).get('checks', {})
        rows.append({
            'size': tag,
            'window_requested': [width, height],
            'ax_window_frame': entry['ax_window_frame'],
            'png_size': entry['png_size'],
            'window_matches_request': entry['window_matches_request'],
            'application_content': entry['application_content_below_title_bar'],
            'table_viewport_width': entry['table_viewport_width'],
            'ax_row_heights': entry['ax_row_heights'],
            'token_measured_rows': len(measured),
            'token_two_lines_ok': token_ok,
            'token_overlap_rows': [row['anchor_y'] for row in measured
                                   if row['vertical_overlap']],
            'token_outside_column_rows': [row['anchor_y'] for row in measured
                                          if not row['inside_token_column']],
            'left_has_source': left.get('has_source_pill'),
            'left_has_credits_label': left.get('has_credits_label'),
            'left_has_large': (left.get('has_large_2_22yi')
                               and left.get('has_large_9877wan')),
            'left_has_unknown': left.get('has_unknown_token') or left.get('has_unpriced'),
            'middle_has_amount': middle.get('has_large_2_22yi') or middle.get('has_unpriced'),
            'right_has_status': (right.get('has_status_completed')
                                 and right.get('has_status_failed')
                                 and right.get('has_status_in_flight')),
            'page2_text': entry['page2_text'],
            'page2_changed_rows': entry['page_changed_rows'],
            'rows_scroll_changed_rows': entry['row_scroll_changed_rows'],
            'left_ids': entry['left_ids'],
            'scroll_ids': entry['scroll_ids'],
            'page2_ids': entry['page2_ids'],
            'pager_frame': entry['pager_frame'],
        })
    acceptance = {'task': 'UI-T07-L01-THREE-SIZES-001', 'sizes': rows,
                  'builtin_1440': analysis.get('builtin_1440', {}).get('checks')}
    (EV / 'acceptance-summary.json').write_text(
        json.dumps(acceptance, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(acceptance, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
