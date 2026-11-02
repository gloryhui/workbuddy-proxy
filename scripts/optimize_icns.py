#!/usr/bin/env python3
"""Recompress ICNS PNG streams without changing pixels, metadata or icon slots."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import zlib


def png_chunks(data):
    if not data.startswith(b'\x89PNG\r\n\x1a\n'):
        raise ValueError('Expected PNG')
    position = 8
    chunks = []
    while position < len(data):
        size = struct.unpack_from('>I', data, position)[0]
        end = position + size + 12
        if end > len(data):
            raise ValueError('Truncated PNG chunk')
        kind = data[position + 4:position + 8]
        payload = data[position + 8:end - 4]
        crc = struct.unpack_from('>I', data, end - 4)[0]
        if zlib.crc32(kind + payload) != crc:
            raise ValueError('PNG CRC mismatch')
        chunks.append((kind, payload, data[position:end]))
        position = end
    if not chunks or chunks[-1][0] != b'IEND':
        raise ValueError('Missing PNG IEND')
    return chunks


def recompress_png(data):
    chunks = png_chunks(data)
    indices = [i for i, (kind, _, _) in enumerate(chunks) if kind == b'IDAT']
    if not indices or indices != list(range(indices[0], indices[-1] + 1)):
        raise ValueError('PNG IDAT chunks must be contiguous')
    filtered_pixels = zlib.decompress(b''.join(chunks[i][1] for i in indices))
    packed = zlib.compress(filtered_pixels, 9)
    idat = struct.pack('>I', len(packed)) + b'IDAT' + packed
    idat += struct.pack('>I', zlib.crc32(b'IDAT' + packed))
    output = bytearray(data[:8])
    for index, (kind, _, raw) in enumerate(chunks):
        if index == indices[0]:
            output.extend(idat)
        elif kind != b'IDAT':
            output.extend(raw)
    output = bytes(output)
    if len(output) >= len(data):
        return data
    verified = png_chunks(output)
    assert [(kind, payload) for kind, payload, _ in chunks if kind != b'IDAT'] == [
        (kind, payload) for kind, payload, _ in verified if kind != b'IDAT']
    assert zlib.decompress(b''.join(payload for kind, payload, _ in verified
                                  if kind == b'IDAT')) == filtered_pixels
    return output


def optimize(data):
    if data[:4] != b'icns' or struct.unpack_from('>I', data, 4)[0] != len(data):
        raise ValueError('Invalid ICNS header')
    position = 8
    entries = []
    rows = []
    while position < len(data):
        kind = data[position:position + 4]
        size = struct.unpack_from('>I', data, position + 4)[0]
        if size < 8 or position + size > len(data):
            raise ValueError('Invalid ICNS entry bounds')
        payload = data[position + 8:position + size]
        packed = recompress_png(payload) if payload.startswith(b'\x89PNG') else payload
        entries.append(kind + struct.pack('>I', len(packed) + 8) + packed)
        rows.append({'slot': kind.decode('ascii'), 'before': size, 'after': len(packed) + 8})
        position += size
    output = b'icns' + struct.pack('>I', 8 + sum(map(len, entries))) + b''.join(entries)
    return output, rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    original = args.source.read_bytes()
    optimized, entries = optimize(original)
    args.output.write_bytes(optimized)
    print(json.dumps({'before_bytes': len(original), 'after_bytes': len(optimized),
                      'before_sha256': hashlib.sha256(original).hexdigest(),
                      'after_sha256': hashlib.sha256(optimized).hexdigest(),
                      'all_slots_preserved': True, 'pixels_and_metadata_unchanged': True,
                      'entries': entries}, indent=2))


if __name__ == '__main__':
    main()
