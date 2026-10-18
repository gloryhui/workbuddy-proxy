"""QA identity of a thin 64-bit Mach-O before/after bundle signing.

Only the signature blob, its final load command/header count, __LINKEDIT size
fields, and trailing zero alignment are ignored. An unsigned input's original
header padding must match the zeroed signature command. All other bytes match.
Callers must separately verify the actual bundle's signature with codesign.
"""
import hashlib
from pathlib import Path
import struct


def normalized_code(raw):
    if len(raw) < 32 or struct.unpack_from('<I', raw)[0] != 0xFEEDFACF:
        raise ValueError('QA identity requires a thin little-endian 64-bit Mach-O')
    count, size = struct.unpack_from('<II', raw, 16)
    end = 32 + size
    if not 0 < count <= 256 or end > len(raw):
        raise ValueError('invalid Mach-O load-command bounds')
    canonical = bytearray(raw)
    offset, signature, signature_command, text_offset = 32, None, None, None
    masks = []
    for _ in range(count):
        if offset + 8 > end:
            raise ValueError('truncated Mach-O load command')
        kind, length = struct.unpack_from('<II', raw, offset)
        if length < 8 or offset + length > end:
            raise ValueError('invalid Mach-O load-command length')
        if kind == 0x19:  # LC_SEGMENT_64
            if length < 72:
                raise ValueError('truncated Mach-O segment')
            name = raw[offset + 8:offset + 24].split(b'\0', 1)[0]
            sections = struct.unpack_from('<I', raw, offset + 64)[0]
            if 72 + sections * 80 > length:
                raise ValueError('truncated Mach-O section table')
            for index in range(sections):
                section = offset + 72 + index * 80
                if name == b'__TEXT' and raw[section:section + 16].split(b'\0', 1)[0] == b'__text':
                    text_offset = struct.unpack_from('<I', raw, section + 48)[0]
            if name == b'__LINKEDIT':
                for field in (32, 48):  # vmsize/filesize include the signature.
                    canonical[offset + field:offset + field + 8] = b'\0' * 8
                    masks.append([offset + field, 8])
        if kind == 0x1D:  # LC_CODE_SIGNATURE
            if signature is not None or length != 16:
                raise ValueError('ambiguous Mach-O signature command')
            signature = struct.unpack_from('<II', raw, offset + 8)
            signature_command = offset
        offset += length
    if offset != end:
        raise ValueError('malformed Mach-O load-command metadata')
    start, signature_size = signature if signature is not None else (len(raw), 0)
    if signature is not None:
        if signature_command != end - 16:
            raise ValueError('signature command must be the final load command')
        if start < end or signature_size <= 0 or start + signature_size > len(raw):
            raise ValueError('invalid Mach-O signature range')
        if any(raw[start + signature_size:]):
            raise ValueError('unexpected nonzero data after the Mach-O signature')
        # codesign adds this command in existing header padding for an unsigned
        # x64 build. Restore its original counts and zero only those 16 bytes;
        # compare the original padding rather than shifting/masking more data.
        canonical[16:24] = struct.pack('<II', count - 1, size - 16)
        canonical[signature_command:end] = b'\0' * 16
        masks.extend([[16, 8], [signature_command, 16]])
    if text_offset is None or not end <= text_offset < start:
        raise ValueError('missing executable text section')
    body = bytes(canonical[:start]).rstrip(b'\0')
    return body, {'sha256': hashlib.sha256(body).hexdigest(), 'load_commands_end': end,
                  'signature_offset': start, 'signature_bytes': signature_size,
                  'signature_command': signature_command,
                  'normalized_bytes': len(body), 'masked_fields': masks,
                  'code_probe_offset': text_offset}


def verify_code_identity(source, runtime):
    original, actual = Path(source).read_bytes(), Path(runtime).read_bytes()
    left, left_info = normalized_code(original)
    right, right_info = normalized_code(actual)
    if not right_info['signature_bytes']:
        raise ValueError('runtime Mach-O must have a signature; verify it separately with codesign')
    if left != right:
        differences = [i for i, (a, b) in enumerate(zip(left, right)) if a != b][:8]
        raise ValueError('signed QA code differs outside signature metadata: %s; sizes=%d/%d'
                         % (differences, len(left), len(right)))
    changed = bytearray(actual)
    probe = right_info['code_probe_offset']
    if probe >= len(right):
        raise ValueError('Mach-O too small for the code-mutation negative control')
    changed[probe] ^= 1
    if normalized_code(changed)[0] == right:
        raise ValueError('code-mutation negative control was not detected')
    signature_only = bytearray(actual)
    signature_only[right_info['signature_offset']] ^= 1
    if normalized_code(signature_only)[0] != right:
        raise ValueError('signature-only normalization control failed')
    return {'source': left_info, 'runtime': right_info, 'normalized_code_equal': True,
            'code_mutation_detected': True, 'signature_only_mutation_ignored': True,
            'input_ui_sha256': hashlib.sha256(original).hexdigest(),
            'runtime_ui_sha256': hashlib.sha256(actual).hexdigest(),
            'comparison_copies_created': False,
            'comparison_copies_removed': True,
            'method': 'all nonsignature bytes; signature metadata/zero alignment masked; no comparison executable copies'}
