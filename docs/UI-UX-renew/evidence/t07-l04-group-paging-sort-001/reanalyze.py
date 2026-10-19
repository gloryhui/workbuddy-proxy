#!/usr/bin/env python3
"""Re-judge the stored T07-L04 GUI evidence after one OCR glyph fix.

The GUI sequence ran once and its screenshots, OCR JSON and AX trees are kept
under this evidence directory. Vision OCR read the simplified "分组" as the
traditional "分組" in one capture (10-providers-size-100), which made the first
parser miss the "25 个分组" total. This script re-parses the *stored* OCR JSON
(no new GUI action, no new screenshot) with a glyph-tolerant total-group regex
and rewrites gui-steps.json.

It also writes gui-vs-api.json: each grouping step's visible row labels against
the matching backend page from core-api-samples.json.
"""
import json
import re
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l04-group-paging-sort-001'
SAMPLES = json.loads((EV / 'core-api-samples.json').read_text(encoding='utf-8'))
STEPS = json.loads((EV / 'gui-steps.json').read_text(encoding='utf-8'))


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
        # Accept the traditional 分組 glyph: Vision occasionally returns it.
        match = re.search(r'(\d+)\s*[个個]分[组組]', text.replace(' ', ''))
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


def matches(state, expect):
    return all(state.get(key) == value for key, value in expect.items())


def api_page_labels(step_name):
    """Return the backend key/label page the GUI step must show, if any."""
    plan = {
        '02-providers-p1-tokens': ('provider', 'tokens', 0),
        '03-providers-p2-tokens': ('provider', 'tokens', 1),
        '04-providers-p2-next-boundary': ('provider', 'tokens', 1),
        '05-providers-back-p1': ('provider', 'tokens', 0),
        '06-providers-sort-requests-p1': ('provider', 'requests', 0),
        '07-providers-p2-requests': ('provider', 'requests', 1),
        '08-providers-sort-tokens-reset-p1': ('provider', 'tokens', 0),
        '09-providers-size-50': ('provider', 'tokens', 0),
        '10-providers-size-100': ('provider', 'tokens', 0),
        '11-providers-size-20': ('provider', 'tokens', 0),
        '12-providers-p2-before-tab': ('provider', 'tokens', 1),
        '13-models-p1-tokens': ('model', 'tokens', 0),
        '14-models-p2-tokens': ('model', 'tokens', 1),
        '15-models-back-p1': ('model', 'tokens', 0),
        '16-models-sort-requests-p1': ('model', 'requests', 0),
        '17-models-p2-requests': ('model', 'requests', 1),
        '18-models-sort-tokens-reset-p1': ('model', 'tokens', 0),
        '19-models-p2-again': ('model', 'tokens', 1),
        '20-providers-p1-after-models': ('provider', 'tokens', 0),
        '23-providers-p1-after-rows-p2': ('provider', 'tokens', 0),
    }
    return plan.get(step_name)


def main():
    reanalysis = {'reason': 'Vision returned the traditional 分組 glyph in one capture',
                  'steps': {}, 'passed': 0, 'total': 0}
    comparison = {'note': 'visible OCR rows vs backend page items (first visible rows)',
                  'steps': {}}
    for record in STEPS['steps']:
        name = record['step']
        ocr_path = EV / record['ocr']
        lines = json.loads(ocr_path.read_text(encoding='utf-8'))
        state = parse(lines)
        record['state'] = state
        record['settled'] = matches(state, record.get('expect') or {})
        record['passed'] = record['settled']
        reanalysis['steps'][name] = {'state': state, 'expect': record.get('expect'),
                                     'passed': record['passed']}
        plan = api_page_labels(name)
        if plan:
            group_by, sort, index = plan
            page = SAMPLES['groups'][group_by]['walk_20'][sort][index]
            expected_labels = []
            for key in page['keys']:
                expected_labels.append(
                    'Provider %02d' % int(key.split('-')[1]) if group_by == 'provider' else key)
            visible = state.get('visible_labels', [])
            comparison['steps'][name] = {
                'group_by': group_by,
                'sort': sort,
                'backend_page': index + 1,
                'backend_keys': page['keys'],
                'backend_labels': expected_labels,
                'visible_ocr_labels': visible,
                'first_matches': (visible[:1] == expected_labels[:1]) if visible else False,
                'visible_is_prefix': expected_labels[:len(visible)] == visible,
                'backend_next_cursor': page['next_cursor'] is not None,
            }
    reanalysis['passed'] = sum(1 for item in reanalysis['steps'].values() if item['passed'])
    reanalysis['total'] = len(reanalysis['steps'])
    STEPS['steps'] = STEPS['steps']
    STEPS['passed'] = reanalysis['passed']
    STEPS['total'] = reanalysis['total']
    STEPS['reanalysis'] = reanalysis
    (EV / 'gui-steps.json').write_text(json.dumps(STEPS, ensure_ascii=False, indent=2),
                                       encoding='utf-8')
    (EV / 'gui-vs-api.json').write_text(json.dumps(comparison, ensure_ascii=False, indent=2),
                                        encoding='utf-8')
    print(json.dumps({'passed': reanalysis['passed'], 'total': reanalysis['total'],
                      'fixed_steps': [name for name, item in reanalysis['steps'].items()
                                      if item['passed'] and name == '10-providers-size-100'],
                      'prefix_mismatches': [name for name, item in comparison['steps'].items()
                                            if not item['visible_is_prefix']]},
                     ensure_ascii=False))


if __name__ == '__main__':
    main()
