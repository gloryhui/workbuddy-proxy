"""In-memory negative controls for the package code-identity boundary."""
import struct
import unittest

from workbuddy_import_macho import normalized_code


def image(signed):
    text = bytearray(152)
    struct.pack_into('<II16s', text, 0, 0x19, len(text), b'__TEXT')
    struct.pack_into('<I', text, 64, 1)
    struct.pack_into('<16s16sQQI', text, 72, b'__text', b'__TEXT', 512, 16, 512)
    link = bytearray(72)
    struct.pack_into('<II16s', link, 0, 0x19, len(link), b'__LINKEDIT')
    struct.pack_into('<QQQ', link, 32, 4096, 1024, 32 if signed else 16)
    uuid = struct.pack('<II16s', 0x1B, 24, b'preserve-this-id')
    commands = bytes(text + link) + uuid
    if signed:
        commands += struct.pack('<IIII', 0x1D, 16, 1040, 16)
    raw = bytearray(1056 if signed else 1040)
    struct.pack_into('<IIIIIIII', raw, 0, 0xFEEDFACF, 0x01000007, 3, 2,
                     4 if signed else 3, len(commands), 0x200085, 0)
    raw[32:32 + len(commands)] = commands
    raw[512:528] = b'executable-code!'
    raw[1024:1040] = b'linkedit-content'
    if signed:
        raw[1040:1056] = b'signature-bytes!'
    return raw


class IdentityTests(unittest.TestCase):
    def test_unsigned_and_signed_match(self):
        self.assertEqual(normalized_code(image(False))[0], normalized_code(image(True))[0])

    def test_code_and_linkedit_and_uuid_mutations_are_detected(self):
        expected = normalized_code(image(False))[0]
        for offset in (512, 1024, 32 + 152 + 72 + 8):
            with self.subTest(offset=offset):
                altered = image(True)
                altered[offset] ^= 1
                self.assertNotEqual(expected, normalized_code(altered)[0])

    def test_nonzero_original_header_padding_is_not_masked(self):
        altered = image(False)
        altered[32 + 152 + 72 + 24] = 1
        self.assertNotEqual(normalized_code(altered)[0], normalized_code(image(True))[0])

    def test_signature_payload_only_is_ignored(self):
        altered = image(True)
        altered[1040] ^= 1
        self.assertEqual(normalized_code(image(True))[0], normalized_code(altered)[0])

    def test_signature_bounds_and_nonzero_trailer_rejected(self):
        for variant in ('outside', 'empty', 'trailer'):
            with self.subTest(variant=variant):
                altered = image(True)
                if variant == 'trailer':
                    altered += b'x'
                else:
                    struct.pack_into('<II', altered, 32 + 152 + 72 + 24 + 8,
                                     1040, 4096 if variant == 'outside' else 0)
                with self.assertRaises(ValueError):
                    normalized_code(altered)

    def test_nonfinal_or_duplicate_signature_commands_rejected(self):
        for duplicate in (False, True):
            with self.subTest(duplicate=duplicate):
                altered = image(True)
                end = 32 + 152 + 72 + 24 + 16
                altered[end:end + 16] = struct.pack('<IIII', 0x1D if duplicate else 0x29,
                                                   16, 1040, 16)
                struct.pack_into('<II', altered, 16, 5, end - 32 + 16)
                with self.assertRaises(ValueError):
                    normalized_code(altered)

    def test_load_command_count_corruption_rejected(self):
        altered = image(True)
        struct.pack_into('<I', altered, 16, 3)
        with self.assertRaises(ValueError):
            normalized_code(altered)


if __name__ == '__main__':
    unittest.main()
