#!/usr/bin/env python3
"""Validate actual native GUI artifacts. This script does not drive the desktop.

Capture window-only images and AX text through the authorized computer-use tool,
then pass --evidence. No screenshots, locale choices or source clicks are faked.
The result covers the recorded remote viewports, not other displays, Windows
or installer acceptance.
"""
import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path

EXPECTED = {'all': (910, '3.42'), 'builtin': (120, '1.14'), 'supplier': (790, '2.28')}
SIZES = {'wide': (1440, 900), 'narrow': (1100, 700)}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def jpeg_size(raw):
    if raw[:2] != b'\xff\xd8':
        raise ValueError('artifact is not an original JPEG image')
    index = 2
    while index + 4 < len(raw):
        if raw[index] != 255:
            raise ValueError('invalid JPEG marker')
        marker = raw[index + 1]
        index += 2
        if marker in (0xD8, 0xD9) or 0xD0 <= marker <= 0xD7:
            continue
        length = struct.unpack('>H', raw[index:index + 2])[0]
        if length < 2 or index + length > len(raw):
            raise ValueError('invalid JPEG segment')
        if marker in (0xC0, 0xC1, 0xC2):
            height, width = struct.unpack('>HH', raw[index + 3:index + 7])
            return width, height
        index += length
    raise ValueError('JPEG dimensions absent')


def verified_file(root, name, artifacts):
    path = (root / name).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ValueError('artifact path escapes evidence directory')
    if artifacts.get(name) != digest(path):
        raise ValueError('artifact hash mismatch: ' + name)
    return path


def matrix(root, core, ui):
    manifest = json.loads((root / 'matrix-manifest.json').read_text())
    binaries = manifest['binary_sha256']
    if binaries.get('review-core') != digest(core):
        raise ValueError('GUI evidence core binary differs from requested current core')
    ui_name = 'WorkBuddy Proxy Review.app/Contents/MacOS/workbuddy-proxy-gpui'
    if 'sandbox_policy_sha256' in manifest and manifest['sandbox_policy_sha256'] != digest(root / 'source-protection.sb'):
        raise ValueError('QA source protection policy differs from manifest')
    if manifest.get('source_guard_library_sha256'):
        if manifest['source_guard_library_sha256'] != digest(root / 'source-guard.dylib'):
            raise ValueError('QA source guard library differs from manifest')
        guarded_pid = int(manifest['sandbox_ui_pid'])
        if guarded_pid <= 0 or (root / 'source-guard-proofs' / ('%d.enforced' % guarded_pid)).read_text() != 'enforced\n':
            raise ValueError('QA actual GUI process has no enforced source protection proof')
    runtime_ui = root / ui_name
    identity = None
    if binaries.get(ui_name) != digest(ui):
        from workbuddy_import_macho import verify_code_identity
        recorded = manifest.get('signature_identity', {})
        if (not manifest.get('complete_bundle_signed')
                or recorded.get('input_ui_sha256') != digest(ui)
                or recorded.get('runtime_ui_sha256') != binaries.get(ui_name)):
            raise ValueError('signed GUI evidence does not identify the requested frontend')
        identity = verify_code_identity(ui, runtime_ui)
        if not identity['normalized_code_equal']:
            raise ValueError('signed GUI nonsignature code differs from requested frontend')
    if digest(root / 'review-core') != digest(core) or digest(runtime_ui) != binaries.get(ui_name):
        raise ValueError('copied runtime binaries differ from their manifest')
    cases = {row['name']: row for row in manifest['checks']}
    if len(cases) != 12 or len(manifest['checks']) != 12:
        raise ValueError('GUI matrix must contain twelve unique cases')
    checks, hashes = [], set()
    ratio = manifest.get('capture_pixel_ratio', 2)
    if ratio not in (1, 2):
        raise ValueError('unsupported original capture pixel ratio')
    # CUA captures the native outer window. macOS may include a one-point
    # bottom frame beyond GPUI's requested content size; retain the original
    # pixels and record that frame explicitly instead of cropping/resizing.
    chrome = manifest.get('window_chrome_height_points', 0)
    if chrome not in (0, 1):
        raise ValueError('unsupported native window frame height')
    for language in ('zh', 'en'):
        for size, logical in SIZES.items():
            for source, (tokens, credits) in EXPECTED.items():
                name = '%s-%s-%s' % (language, size, source)
                case = cases[name]
                # A native restore-size action can remove the one-point bottom
                # frame within one session. Keep each original capture and
                # permit only that documented frame variation, never scaling.
                case_chrome = case.get('window_chrome_height_points', chrome)
                if case_chrome not in (0, 1):
                    raise ValueError('unsupported native case window frame height')
                dimensions = (logical[0] * ratio, (logical[1] + case_chrome) * ratio)
                image = verified_file(root, name + '.jpg', manifest['artifacts'])
                text = verified_file(root, name + '.ax.txt', manifest['artifacts']).read_text()
                if jpeg_size(image.read_bytes()) != dimensions or tuple(case['native_image_size']) != dimensions:
                    raise ValueError('native dimensions differ: ' + name)
                labels = ('Token 总用量 %d' % tokens, 'WorkBuddy 点数 ' + credits) if language == 'zh' else (
                    'Total tokens %d' % tokens, 'WorkBuddy credits ' + credits)
                if not all(label in text for label in labels):
                    raise ValueError('actual AX metric assertion failed: ' + name)
                if case['tokens'] != tokens or case['credits'] != credits:
                    raise ValueError('case expected metrics differ: ' + name)
                hashes.add(digest(image))
                checks.append({'case': name, 'tokens': tokens, 'credits': credits, 'size': dimensions})
    if len(hashes) != 12:
        raise ValueError('matrix contains repeated screenshot bytes')
    return {'checks': checks, 'scope': manifest['scope'], 'limitations': manifest['limitations'],
            'binary_sha256': binaries, 'input_ui_sha256': digest(ui),
            'signed_ui_code_identity': identity,
            'manifest_sha256': digest(root / 'matrix-manifest.json')}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', '--core', dest='exe', default='target/release/workbuddy-proxy')
    parser.add_argument('--ui', default='gpui/target/release/workbuddy-proxy-gpui')
    parser.add_argument('--evidence', help='native CUA matrix evidence directory')
    parser.add_argument('--output', default='dist/workbuddy-import-validation/S09.08-current')
    args = parser.parse_args()
    report = {'result': 'NOT_RUN', 'passed': 0, 'failed': 0,
              'remaining_gates': ['current remote viewport interaction evidence aggregation', 'Windows GUI', 'installer acceptance',
                                  'mapping/expiry/CAS and pagination interaction artifacts']}
    if args.evidence:
        try:
            report.update(matrix(Path(args.evidence), Path(args.exe), Path(args.ui)))
            report.update(result='MATRIX_VERIFIED_OTHER_GATES_PENDING', passed=12)
        except (OSError, ValueError, KeyError, TypeError, struct.error) as failure:
            report.update(result='FAIL', failed=1, error=str(failure))
    else:
        report['reason'] = 'Native window-only screenshots and AX event evidence must be captured first.'
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    (output / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print('%s: %d matrix cases verified' % (report['result'], report['passed']))
    return 0 if report['passed'] == 12 else 1


if __name__ == '__main__':
    sys.exit(main())
