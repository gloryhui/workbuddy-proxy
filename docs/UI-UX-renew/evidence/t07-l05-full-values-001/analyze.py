#!/usr/bin/env python3
"""Column-cropped OCR extraction for UI-T07-L05-FULL-VALUES-001.

Every stored screenshot is cut into cells and re-OCR'd: the table header gives
the column x-boxes, and the time column's own OCR text gives each row's y-box
(the screenshot is window-local while AX frames are screen-local, so the time
text is the one anchor that cannot drift). Row identity comes from that time
text; a row whose time cell failed OCR is interpolated from its neighbour,
because the log is strictly time-ordered.

Outputs (inside the evidence directory):
  observed-<label>.json   every OCR reading per row and column plus layouts
  cells-<label>.json      the raw cell texts, for manual review

usage: analyze.py [--labels pre-t07,current] [--workers 6]
"""
import argparse
import json
import re
import subprocess
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l05-full-values-001'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/'
           '01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l05-full-values-001')
OCR = CAP / 'bin/ocr'
CROPS = CAP / 'crops'
RAW = EV / 'raw'
TIME_RE = re.compile(r'10-\d{2}\s*\d{2}:\d{2}:\d{2}')

COLUMNS = {
    'pre-t07': ['时间', '来源', '供应商', '模型', '请求标识', 'Token/缓存命中率',
                '估算费用', '耗时', 'tok/s', '状态', '点数'],
    'current': ['时间', '供应商', '模型', '请求标识', 'Token/缓存命中',
                '估算费用', '耗时', 'tok/s', '状态'],
}
COLUMN_FIELD = {
    '时间': 'time', '来源': 'source', '供应商': 'provider', '模型': 'model',
    '请求标识': 'request_id_shown', 'Token/缓存命中率': 'tokens',
    'Token/缓存命中': 'tokens', '估算费用': 'cost', '耗时': 'latency',
    'tok/s': 'speed', '状态': 'status_cell', '点数': 'credit',
}

TRADITIONAL = str.maketrans({
    '敗': '败', '緩': '缓', '讀': '读', '寫': '写', '總': '总', '點': '点',
    '內': '内', '進': '进', '斷': '断', '錄': '录', '誤': '误', '連': '连',
    '結': '结', '錯': '错', '費': '费', '價': '价', '單': '单', '確': '确',
    '預': '预', '計': '计', '狀': '状', '態': '态', '數': '数', '據': '据',
    '發': '发', '刪': '删', '時': '时', '間': '间', '來': '来', '應': '应',
    '為': '为', '過': '过', '還': '还', '這': '这', '裡': '里', '體': '体',
    '顯': '显', '額': '额', '歷': '历', '險': '险', '證': '证', '設': '设',
    '備': '备', '環': '环', '靜': '静', '啟': '启', '動': '动', '網': '网',
})


def normalize(text):
    value = text.strip().replace('\u3000', ' ')
    value = value.translate(TRADITIONAL)
    value = re.sub(r'\s+', '', value)
    value = re.sub(r'[•・‧·]', '·', value)
    value = value.replace('，', '·').replace(',', '·')
    return value


def normalize_number(text):
    value = normalize(text)
    value = value.replace('O', '0').replace('o', '0')
    return value


def header_boxes(ocr_lines):
    candidates = [line for line in ocr_lines
                  if line.get('confidence', 0) >= 0.2
                  and line['text'].strip() == '时间'
                  and 320 <= line['y'] <= 640]
    if not candidates:
        return None, []
    header = sorted(candidates, key=lambda line: line['y'])[0]
    labels = [line for line in ocr_lines
              if line.get('confidence', 0) >= 0.2
              and abs(line['y'] - header['y']) <= 16
              and line['x'] > header['x'] - 4]
    labels.sort(key=lambda line: line['x'])
    merged = []
    for line in labels:
        if merged and line['x'] - merged[-1]['x'] <= 26:
            merged[-1] = {**merged[-1],
                          'w': max(merged[-1]['w'],
                                   line['x'] + line['w'] - merged[-1]['x'])}
            continue
        merged.append(dict(line))
    return header, merged


def column_boxes(labels, table_x0, table_x1, expected_columns):
    boxes = {}
    for index, title in enumerate(expected_columns):
        match = None
        for item in labels:
            if normalize(item['text']) == normalize(title):
                match = item
                break
        if match is None:
            for item in labels:
                if normalize(title)[:3] in normalize(item['text']):
                    match = item
                    break
        if match is None:
            continue
        boxes[title] = max(table_x0, match['x'] - 12)
    ordered = [title for title in expected_columns if title in boxes]
    result = {}
    for index, title in enumerate(ordered):
        right = (boxes[ordered[index + 1]] - 1
                 if index + 1 < len(ordered) else table_x1)
        result[title] = [boxes[title], right]
    return result


def parse_rows(path):
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


def crop_and_ocr(args):
    png, box, tag = args
    from PIL import Image
    x0, y0, x1, y1 = box
    if x1 - x0 < 4 or y1 - y0 < 4:
        return tag, ''
    CROPS.mkdir(parents=True, exist_ok=True)
    path = CROPS / ('%s.png' % tag)
    with Image.open(png) as image:
        cell = image.crop((x0, y0, x1, y1))
        cell = cell.resize(((x1 - x0) * 3, (y1 - y0) * 3), Image.LANCZOS)
        cell.save(path)
    result = subprocess.run([str(OCR), str(path), 'zh-Hans,en-US'],
                            capture_output=True, text=True, timeout=120)
    try:
        lines = json.loads(result.stdout)
    except json.JSONDecodeError:
        return tag, ''
    lines.sort(key=lambda line: (line['y'], line['x']))
    return tag, ' '.join(line['text'] for line in lines).strip()


def read_screen(label, screen, expected_columns, expected_times, executor):
    png = RAW / label / 'shots' / (screen + '.png')
    ax_path = RAW / label / 'ax' / (screen + '.txt')
    ocr_path = RAW / label / 'ocr' / (screen + '.json')
    if not png.exists() or not ax_path.exists() or not ocr_path.exists():
        return None
    ocr_lines = json.loads(ocr_path.read_text(encoding='utf-8'))
    frames = parse_rows(ax_path)
    header, labels = header_boxes(ocr_lines)
    table_x0 = min((row['x'] for row in frames), default=311)
    table_x1 = max((row['x'] + row['w'] for row in frames), default=1638)
    boxes = column_boxes(labels, table_x0, table_x1, expected_columns)
    anchors = []
    seen_y = set()
    for line in ocr_lines:
        if line.get('confidence', 0) < 0.2:
            continue
        match = TIME_RE.search(line['text'])
        if not match or line['x'] > 430:
            continue
        if abs(line['y'] - max(seen_y, default=-99)) <= 4:
            continue
        seen_y.add(line['y'])
        anchors.append({'y': line['y'], 'text': match.group(0)})
    anchors.sort(key=lambda item: item['y'])
    gaps = [b['y'] - a['y'] for a, b in zip(anchors, anchors[1:])
            if 20 <= b['y'] - a['y'] <= 120]
    row_h = sorted(gaps)[len(gaps) // 2] if gaps else (
        frames[0]['h'] if frames else 30)
    jobs = []
    cells = []
    for index, anchor in enumerate(anchors):
        top = max(0, anchor['y'] - 8)
        bottom = top + row_h
        entry = {'index': index, 'time_text': anchor['text'], 'top': top,
                 'height': row_h, 'cells': {}}
        for title, (x0, x1) in boxes.items():
            tag = '%s-%s-%02d-%s' % (
                label, screen, index,
                re.sub(r'[^0-9A-Za-z\u4e00-\u9fff]', '_', title))
            jobs.append((str(png), (x0, top, x1, bottom), tag))
            entry['cells'][title] = tag
        cells.append(entry)
    results = {}
    if jobs:
        for tag, text in executor.map(crop_and_ocr, jobs, chunksize=8):
            results[tag] = text
    for entry in cells:
        for title, tag in entry['cells'].items():
            entry['cells'][title] = results.get(tag, '')
    return {
        'screen': screen, 'header_y': header['y'] if header else None,
        'labels': [{'text': item['text'], 'x': item['x']} for item in labels],
        'boxes': boxes, 'row_height': row_h, 'cells': cells,
        'row_frames': frames,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--labels', default='pre-t07,current')
    parser.add_argument('--workers', type=int, default=6)
    args = parser.parse_args()
    labels = args.labels.split(',')
    expected = json.loads((EV / 'expected.json').read_text(encoding='utf-8'))
    expected_times = {normalize_number(row['expected']['time']): row['id']
                      for row in expected['rows']}
    summary = {'labels': labels, 'views': {}, 'observations': {}}

    with ProcessPoolExecutor(max_workers=args.workers) as executor:
        for label in labels:
            capture = json.loads((RAW / label / 'capture.json').read_text(
                encoding='utf-8'))
            observed = {}
            all_cells = {}
            layouts = []
            for view, screens in capture['views'].items():
                for screen in screens:
                    data = read_screen(label, screen, COLUMNS[label],
                                       expected_times, executor)
                    if data is None:
                        continue
                    layouts.append({key: data[key] for key in
                                    ('screen', 'header_y', 'labels', 'boxes',
                                     'row_height')})
                    all_cells[screen] = data['cells']
                    resolved = []
                    last_id = None
                    for entry in data['cells']:
                        row_id = expected_times.get(
                            normalize_number(entry['time_text']))
                        if row_id is None and last_id is not None:
                            row_id = last_id - 1
                        resolved.append(row_id)
                        if row_id is not None:
                            last_id = row_id
                    for entry, row_id in zip(data['cells'], resolved):
                        if row_id is None:
                            continue
                        observed.setdefault(str(row_id), {})
                        for title, text in entry['cells'].items():
                            if not text:
                                continue
                            field = COLUMN_FIELD.get(title, title)
                            bucket = observed[str(row_id)].setdefault(field, [])
                            if text not in bucket:
                                bucket.append(text)
            (EV / ('observed-%s.json' % label)).write_text(
                json.dumps({'label': label, 'observations': observed,
                            'layouts': layouts}, ensure_ascii=False, indent=1),
                encoding='utf-8')
            (EV / ('cells-%s.json' % label)).write_text(
                json.dumps(all_cells, ensure_ascii=False, indent=1),
                encoding='utf-8')
            summary['observations'][label] = {
                'rows_seen': sorted(observed.keys(), key=int),
                'screens': len(layouts),
            }
            summary['views'][label] = capture['views']
            print('%s: rows seen %s' % (label, sorted(observed.keys(), key=int)),
                  flush=True)

    (EV / 'compare-summary.json').write_text(
        json.dumps(summary, ensure_ascii=False, indent=2), encoding='utf-8')
    print('wrote observed-*.json, cells-*.json, compare-summary.json')


if __name__ == '__main__':
    main()
