"""Read the shared Core/UI release version without machine-specific defaults."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
VERSION = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\Z')


def manifest_version(path):
    # Only the package section is needed. Keep packaging compatible with Python
    # 3.9 too, without a new TOML dependency or matching dependency versions.
    text = Path(path).read_text(encoding='utf-8')
    section = re.search(r'^\[package\]\s*\n(.*?)(?=^\[|\Z)', text, re.M | re.S)
    matches = re.findall(r'^version\s*=\s*"([^"\n]+)"\s*(?:#.*)?$',
                         section[1] if section else '', re.M)
    if len(matches) != 1 or not VERSION.fullmatch(matches[0]):
        raise ValueError(f'{path}: expected one numeric package version (major.minor.patch)')
    return matches[0]


def project_version(root=ROOT):
    root = Path(root)
    core = manifest_version(root / 'Cargo.toml')
    ui = manifest_version(root / 'gpui/Cargo.toml')
    if core != ui:
        raise ValueError(f'Core version {core} differs from UI version {ui}')
    return core


def release_version(requested=None, root=ROOT):
    version = project_version(root)
    if requested is not None and requested != version:
        raise ValueError(f'Package version {requested} must match Core/UI version {version}')
    return version


if __name__ == '__main__':
    print(project_version())
