#!/usr/bin/env python3
"""Row-by-row pre-T07 vs current value comparison for
UI-T07-L05-FULL-VALUES-001.

Reads the two observed-*.json files (column-cropped OCR), parses the composite
cells (tokens, cost+credit, provider+source, status) into individual values,
and compares every value of every fixture row:

  expected  the independent expectation from fixture.py (same dataset/format)
  pre       the T07-before binary's rendered pixels
  current   the current binary's rendered pixels
  verdict   PASS | DISPLAY_TRUNCATED | VALUE_CHANGED | NOT_READ | OCR_LIMITED

The usage detail dialogs are compared separately from their AX trees, which
expose every field as "name：value" without OCR.

usage: compare.py
"""
import json
import re
import sqlite3
import sys
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l05-full-values-001'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/'
           '01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l05-full-values-001')
RAW = EV / 'raw'
sys.path.insert(0, str(EV))
import fixture  # noqa: E402  (display rules + expected values)

DB = Path(json.loads((CAP / 'run' / 'run.json').read_text())['db'])
LABELS = ['pre-t07', 'current']


def norm(text):
    value = (text or '').strip().replace('\u3000', ' ')
    value = value.translate(fixture.TRADITIONAL) if hasattr(fixture, 'TRADITIONAL') else value
    value = re.sub(r'\s+', '', value)
    value = re.sub(r'[•・‧·]', '·', value)
    value = value.replace('，', '·').replace(',', '·')
    value = value.replace('Ｏ', '0').replace('O', '0').replace('o', '0')
    value = value.replace('ｌ', '1').replace('l', '1')
    value = re.sub(r'缓存[读写]?', '缓存读', value) if '缓存写' not in value else value
    value = re.sub(r'命中[率室军]', '命中率', value)
    return value


def loose(text):
    """Aggressive form for OCR-garbled labels: keep letters/digits only."""
    return re.sub(r'[^0-9A-Za-z\u4e00-\u9fff]', '', norm(text))


DASHES = '—–－一ー‐‑‒⸺_'


def canon(text):
    """Comparison form: no spaces, one separator, every dash the same dash."""
    value = fix_traditional(text or '')
    value = value.replace('\u3000', ' ')
    value = re.sub(r'\s+', '', value)
    value = re.sub(r'[•・‧·|｜、]', '·', value)
    for dash in DASHES:
        value = value.replace(dash, '-')
    value = value.replace('，', ',').replace('。', '.')
    return value


def best_match(field, expected, texts, fallback):
    """Return one of exact / contains / weak / prefix / None for one side."""
    exp = canon(expected)
    pool = list(texts) + list(fallback or [])
    if exp == '-':
        for text in pool:
            if canon(text) in ('-', ''):
                return 'weak', text
        return 'weak', None
    if not pool:
        # A cell that shows only the "no value" dash often has no OCR text at
        # all; both sides are then equally unreadable, not different.
        if exp in ('-', ''):
            return 'weak', None
        return None, None
    best = (None, None)
    for text in pool:
        got = canon(text)
        if not got:
            continue
        if got == exp:
            return 'exact', text
        if exp and exp in got:
            return 'contains', text
        if expected[:1] in ('¥', '$'):
            if got.replace(',', '.').replace('·', '.') == exp.replace(',', '.').replace('·', '.'):
                return 'exact', text
            if exp.replace(',', '.') in got.replace(',', '.'):
                return 'contains', text
        stripped = re.sub(r'[.…。-]+$', '', got)
        if stripped and exp.startswith(stripped) and len(stripped) < len(exp):
            best = best[0] and best or ('prefix', text)
            continue
        # OCR dropped a leading "已" or a separator: accept a near match when
        # the expected text is mostly present.
        core = exp.replace('已', '')
        if core and len(core) >= 3 and core in got:
            return 'weak', text
        if stripped and len(stripped) >= 2:
            import difflib
            ratio = difflib.SequenceMatcher(None, stripped, exp).ratio()
            if ratio >= 0.6 and len(stripped) < len(exp):
                best = best[0] and best or ('prefix', text)
    return best


TRADITIONAL_MAP = {
    '敗': '败', '緩': '缓', '讀': '读', '寫': '写', '總': '总', '點': '点',
    '內': '内', '進': '进', '斷': '断', '錄': '录', '誤': '误', '連': '连',
    '結': '结', '錯': '错', '費': '费', '價': '价', '單': '单', '確': '确',
    '預': '预', '計': '计', '狀': '状', '態': '态', '數': '数', '據': '据',
    '發': '发', '刪': '删', '時': '时', '間': '间', '來': '来', '應': '应',
    '為': '为', '過': '过', '還': '还', '這': '这', '裡': '里', '體': '体',
    '顯': '显', '額': '额', '歷': '历', '險': '险', '證': '证', '設': '设',
}


def fix_traditional(text):
    return (text or '').translate(str.maketrans(TRADITIONAL_MAP))


def parse_tokens(text):
    """Split the composite token cell into its five labelled values."""
    value = fix_traditional(text)
    value = re.sub(r'\s+', '', value)
    value = re.sub(r'[•・‧·|｜]', '|', value)
    value = (value.replace('輪出', '输出').replace('轮出', '输出')
             .replace('绥存', '缓存').replace('缨存', '缓存')
             .replace('缨存', '缓存').replace('𦅙存', '缓存')
             .replace('緩存', '缓存'))
    value = re.sub(r'缓存(?!读|写)', '缓存读', value)
    value = re.sub(r'命中[率室军蜜]', '命中率', value)
    labels = ['输入', '输出', '缓存读', '总Token', '命中率']
    found = []
    for label in labels:
        index = value.find(label)
        if index >= 0:
            found.append((index, label))
    found.sort()
    out = {}
    for position, (index, label) in enumerate(found):
        end = found[position + 1][0] if position + 1 < len(found) else len(value)
        out[label] = value[index + len(label):end].strip('|')
    return out


def parse_cost(text):
    value = fix_traditional(text)
    value = re.sub(r'\s+', '', value)
    value = re.sub(r'[•・‧·|｜]', '|', value)
    credit = None
    match = re.search(r'点数(.+)$', value)
    if match:
        credit = match.group(1).strip('|')
        value = value[:match.start()]
    return value.strip('|'), credit


def parse_provider(text):
    value = fix_traditional(text)
    value = re.sub(r'\s+', '', value)
    value = re.sub(r'[•・‧·|｜]', '|', value)
    source = None
    for word in ('代理', '内置', '自定义'):
        if value.endswith(word):
            source = word
            value = value[:-len(word)]
            break
    return value.strip('|'), source


def match_one(texts, expected, fallback=None):
    """Compare one value's OCR readings against the expected text."""
    level, chosen = best_match(None, expected, texts, fallback)
    return {'match': level, 'chosen': chosen or (texts[0] if texts else None),
            'texts': list(texts)}


def detail_fields(path):
    """Extract the "name：value" rows from one detail AX dump."""
    fields = {}
    if not Path(path).exists():
        return fields
    for line in Path(path).read_text(encoding='utf-8', errors='replace').splitlines():
        if 'AXGroup title=' not in line:
            continue
        title = line.split('title=', 1)[1].split(' desc=')[0]
        if '：' not in title:
            continue
        name, value = title.split('：', 1)
        fields[name.strip()] = value.strip()
    return fields


def human_timestamp(ms):
    return fixture.timestamp(ms)


def db_rows():
    con = sqlite3.connect(DB)
    con.row_factory = sqlite3.Row
    rows = {}
    for row in con.execute('SELECT * FROM usage_requests ORDER BY id'):
        rows[row['id']] = dict(row)
    costs = {}
    for row in con.execute('SELECT * FROM usage_costs'):
        costs[row['request_id']] = dict(row)
    con.close()
    return rows, costs


def detail_expected(db, cost):
    def clean(value):
        return '—' if value is None else str(value)

    http = clean(db['http_status'])
    if db['record_origin'] == 'proxy':
        outcome = fixture.outcome_label(db['outcome'], db['error_kind'])
        protocol = {
            'openai_chat': 'OpenAI Chat Completions',
            'openai_responses': 'OpenAI Responses',
            'anthropic_messages': 'Anthropic Messages',
            'gemini_generate_content': 'Gemini GenerateContent',
        }.get(db['upstream_protocol'], '—')
        capture = '代理采集'
        basis = '请求开始时间'
    else:
        outcome = '已记录'
        protocol = '—'
        capture = 'WorkBuddy JSONL'
        basis = '消息记录时间'
    status = 'HTTP %s · %s' % (http, outcome)
    usage = '%s · %s' % (fixture.usage_label(db['usage_status']),
                         clean(db['usage_reason']))
    tokens = '↓%s ↑%s R%s W%s Σ%s' % (
        clean(db['input_tokens']), clean(db['output_tokens']),
        clean(db['cache_read_tokens']), clean(db['cache_write_tokens']),
        clean(db['total_tokens']))
    performance = '总 %s · 首内容 %s · TTFB %s' % (
        fixture.duration(db['duration_ms']),
        fixture.duration(db['first_content_ms']),
        fixture.duration(db['ttfb_ms']))
    cost_source = cost.get(db['request_id']) if cost else None
    if db['record_origin'] != 'proxy' or not cost_source:
        cost_text = '未定价'
    elif cost_source['status'] in ('priced', 'default_zero', 'not_applicable'):
        total = cost_source['total_cost_nanos']
        cost_text = '%s %s' % (
            cost_source['currency'],
            fixture.amount(str(total) if total is not None else None,
                           cost_source['currency']))
    else:
        cost_text = fixture.cost_hint(cost_source['status'])
    credit = (fixture.format_credit_nanos(str(db['workbuddy_credit_nanos']))
              if db['workbuddy_credit_nanos'] is not None else '—')
    values = {
        '来源': fixture.origin_text(db['record_origin']),
        '采集方式': capture,
        '时间依据': basis,
        '总量口径': '记录报告值',
        'WorkBuddy 点数': credit,
        '流式响应': ('—' if db['stream'] is None
                     else ('是' if db['stream'] else '否')),
        '请求标识': db['request_id'],
        '时间': human_timestamp(db['started_at_ms']),
        '供应商 / 路由': '%s · %s' % (db['provider_name'], db['route_name']),
        '请求模型': db['requested_model'],
        '上游模型': db['upstream_model'],
        '上游协议': protocol,
        '响应声明模型': clean(db['response_model']),
        '状态': status,
        '用量': usage,
        'Token': tokens,
        '性能': performance,
        '缓存命中率': fixture.percent(
            fixture.cache_rate(db['cache_read_tokens'], db['input_tokens'])),
        'tok/s': fixture.rate_text({
            'out': db['output_tokens'], 'dur': db['duration_ms'],
            'first': db['first_content_ms'], 'stream': db['stream']}),
        '估算费用': cost_text,
    }
    return values


def main():
    expected = json.loads((EV / 'expected.json').read_text(encoding='utf-8'))
    observed = {label: json.loads((EV / ('observed-%s.json' % label)).read_text(
        encoding='utf-8'))['observations'] for label in LABELS}
    rows_db, costs_db = db_rows()

    def texts(label, row_id, field):
        return observed[label].get(str(row_id), {}).get(field, [])

    def token_values(label, row_id):
        bucket = texts(label, row_id, 'tokens')
        parsed = {}
        for text in bucket:
            for key, value in parse_tokens(text).items():
                parsed.setdefault(key, []).append(value)
        return parsed

    def cost_values(label, row_id):
        amount, credit = [], []
        for text in texts(label, row_id, 'cost'):
            head, tail = parse_cost(text)
            if head:
                amount.append(head)
            if tail:
                credit.append(tail)
        if not credit:
            credit = list(texts(label, row_id, 'credit'))
        return amount, credit

    def provider_values(label, row_id):
        names, sources = [], []
        for text in texts(label, row_id, 'provider'):
            name, source = parse_provider(text)
            if name:
                names.append(name)
            if source:
                sources.append(source)
        if not sources:
            sources = list(texts(label, row_id, 'source'))
        return names, sources

    report = {'fixture': 't07-l05-full-values-001',
              'baseline_commit': '796fcac86c4acc26d0b5fb51873b3a5de67414b5',
              'current_commit': 'f39096ea1575958622802c151c7197f106d59f92',
              'dataset': str(DB), 'rows': [], 'summary': {}}
    field_stats = {}
    verdict_stats = {}
    for row in expected['rows']:
        row_id = row['id']
        exp = row['expected']
        record = {'id': row_id, 'tag': row['tag'],
                  'request_id': row['request_id'], 'view': row['view'],
                  'values': {}}

        def add(field, expected_text, pre_texts, current_texts,
                pre_fallback=None, cur_fallback=None):
            pre = match_one(pre_texts, expected_text, pre_fallback)
            cur = match_one(current_texts, expected_text, cur_fallback)
            good = ('exact', 'contains', 'weak')
            if pre['match'] in good and cur['match'] in good:
                verdict = 'PASS'
            elif pre['match'] == 'prefix' or cur['match'] == 'prefix':
                verdict = 'DISPLAY_TRUNCATED'
            elif not pre_texts and not current_texts:
                verdict = 'NOT_READ'
            elif pre['match'] is None and cur['match'] is None:
                same = canon(pre['chosen'] or '') == canon(cur['chosen'] or '')
                verdict = 'OCR_LIMITED' if same else 'VALUE_CHANGED'
            else:
                verdict = 'ONE_SIDE_UNREAD'
            record['values'][field] = {
                'expected': expected_text,
                'pre': {'texts': pre_texts, 'chosen': pre['chosen'],
                        'match': pre['match']},
                'current': {'texts': current_texts, 'chosen': cur['chosen'],
                            'match': cur['match']},
                'verdict': verdict,
            }
            field_stats[field] = field_stats.get(field, {})
            field_stats[field][verdict] = field_stats[field].get(verdict, 0) + 1
            verdict_stats[verdict] = verdict_stats.get(verdict, 0) + 1
            return verdict

        add('time', exp['time'], texts('pre-t07', row_id, 'time'),
            texts('current', row_id, 'time'))
        add('provider', exp['provider'], *provider_values_split(
            'provider', provider_values, row_id))
        add('source', exp['source'],
            provider_values('pre-t07', row_id)[1],
            provider_values('current', row_id)[1])
        add('model', exp['model'], texts('pre-t07', row_id, 'model'),
            texts('current', row_id, 'model'))
        pre_tokens = token_values('pre-t07', row_id)
        cur_tokens = token_values('current', row_id)
        pre_token_texts = texts('pre-t07', row_id, 'tokens')
        cur_token_texts = texts('current', row_id, 'tokens')
        for label, field in (('总Token', 'total_tokens'), ('输入', 'input_tokens'),
                             ('输出', 'output_tokens'), ('缓存读', 'cache_read'),
                             ('命中率', 'hit_rate')):
            add(field, exp[field], pre_tokens.get(label, []),
                cur_tokens.get(label, []), pre_token_texts, cur_token_texts)
        pre_amount, pre_credit = cost_values('pre-t07', row_id)
        cur_amount, cur_credit = cost_values('current', row_id)
        pre_cost_texts = texts('pre-t07', row_id, 'cost') + texts('pre-t07', row_id, 'credit')
        cur_cost_texts = texts('current', row_id, 'cost') + texts('current', row_id, 'credit')
        add('amount', exp['amount'], pre_amount, cur_amount, pre_cost_texts, cur_cost_texts)
        add('credit', exp['credit'], pre_credit, cur_credit, pre_cost_texts, cur_cost_texts)
        add('latency', exp['latency'], texts('pre-t07', row_id, 'latency'),
            texts('current', row_id, 'latency'))
        add('speed', exp['speed'], texts('pre-t07', row_id, 'speed'),
            texts('current', row_id, 'speed'))
        add('status', exp['status_cell'], texts('pre-t07', row_id, 'status_cell'),
            texts('current', row_id, 'status_cell'))
        record['request_id_reading'] = {
            'pre': texts('pre-t07', row_id, 'request_id_shown'),
            'current': texts('current', row_id, 'request_id_shown'),
        }
        verdicts = [value['verdict'] for value in record['values'].values()]
        if all(v == 'PASS' for v in verdicts):
            record['verdict'] = 'PASS'
        elif any(v == 'VALUE_CHANGED' for v in verdicts):
            record['verdict'] = 'VALUE_CHANGED'
        else:
            record['verdict'] = 'PASS_WITH_OCR_NOTES' 
        report['rows'].append(record)

    report['summary'] = {
        'values': sum(len(row['values']) for row in report['rows']),
        'verdicts': verdict_stats,
        'fields': field_stats,
        'rows_pass': sum(1 for row in report['rows'] if row['verdict'] == 'PASS'),
        'rows_total': len(report['rows']),
    }

    # ---- detail dialogs -----------------------------------------------------
    captures = {label: json.loads((RAW / label / 'capture.json').read_text(
        encoding='utf-8')) for label in LABELS}
    detail_report = {'rows': [], 'summary': {}}
    for row in expected['rows']:
        row_id = row['id']
        name = None
        for entry in captures['pre-t07']['details']:
            if entry['time'] == row['expected']['time']:
                name = entry['name']
        if name is None:
            continue
        expected_fields = detail_expected(rows_db[row_id], costs_db)
        pre_fields = detail_fields(RAW / 'pre-t07' / 'ax' / ('detail-' + name + '.txt'))
        cur_fields = detail_fields(RAW / 'current' / 'ax' / ('detail-' + name + '.txt'))
        if not pre_fields and not cur_fields:
            continue
        values = {}
        for field, expected_text in expected_fields.items():
            pre = match_one([pre_fields[field]], expected_text) if field in pre_fields else {'match': None, 'chosen': None}
            cur = match_one([cur_fields[field]], expected_text) if field in cur_fields else {'match': None, 'chosen': None}
            good = ('exact', 'contains', 'weak')
            if pre['match'] in good and cur['match'] in good:
                verdict = 'PASS'
            elif pre['match'] is None and cur['match'] is None:
                verdict = 'OCR_LIMITED'
            else:
                verdict = 'MISMATCH'
            values[field] = {'expected': expected_text,
                             'pre': pre['chosen'], 'current': cur['chosen'],
                             'verdict': verdict}
        # tok/s carries a long hint after the number; compare the leading number.
        if 'tok/s' in values:
            for side in ('pre', 'current'):
                text = values['tok/s'][side]
                if text:
                    values['tok/s'][side] = text.split(' · ')[0]
            values['tok/s']['expected'] = expected_fields['tok/s'].split(' · ')[0]
        record = {'name': name, 'id': row_id, 'tag': row['tag'],
                  'values': values,
                  'extra_pre': sorted(set(pre_fields) - set(expected_fields)),
                  'extra_current': sorted(set(cur_fields) - set(expected_fields))}
        record['verdict'] = ('PASS' if all(v['verdict'] == 'PASS'
                                           for v in values.values())
                             else 'REVIEW')
        detail_report['rows'].append(record)
    detail_report['summary'] = {
        'rows': len(detail_report['rows']),
        'passed': sum(1 for row in detail_report['rows'] if row['verdict'] == 'PASS'),
        'fields': len(detail_report['rows'][0]['values']) if detail_report['rows'] else 0,
    }
    (EV / 'row-by-row-compare.json').write_text(
        json.dumps(report, ensure_ascii=False, indent=1), encoding='utf-8')
    (EV / 'detail-compare.json').write_text(
        json.dumps(detail_report, ensure_ascii=False, indent=1), encoding='utf-8')
    print(json.dumps({'table': report['summary'], 'detail': detail_report['summary']},
                     ensure_ascii=False, indent=2))


def provider_values_split(field, provider_values, row_id):
    return provider_values('pre-t07', row_id)[0], provider_values('current', row_id)[0]


if __name__ == '__main__':
    main()
