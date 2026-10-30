"""Build the Windows portable update protocol. Developer/CI tool; no Python runtime in Lyra."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path
import tomllib
import zipfile

MANAGED = ('lyra-desktop.exe', 'lyra.exe', 'THIRD_PARTY_NOTICES.txt', 'VERSION')
ASSET = 'lyra-windows-x86_64.zip'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dist', type=Path, default=Path('dist'))
    parser.add_argument('--output', type=Path, default=Path('release'))
    parser.add_argument('--verify-executables', action='store_true')
    args = parser.parse_args()
    version = tomllib.loads(Path('Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    if args.verify_executables:
        for name, options in [('lyra.exe', ['--version']), ('lyra-desktop.exe', ['--worker', '--version'])]:
            result = subprocess.run([str((args.dist / name).resolve()), *options], capture_output=True, text=True, timeout=10, check=True)
            if result.stdout.strip() != 'lyra ' + version:
                raise ValueError(f'Executable version mismatch: {name}')
    (args.dist / 'VERSION').write_text(version + '\n', encoding='utf-8')
    files = {}
    for name in MANAGED:
        path = args.dist / name
        with path.open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        files[name] = {'size': path.stat().st_size, 'sha256': digest}
    manifest = {'schema_version': 1, 'version': version, 'target': 'x86_64-pc-windows-msvc', 'flavor': 'cpu', 'files': files}
    (args.dist / 'lyra-update.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    args.output.mkdir(parents=True, exist_ok=True)
    archive = args.output / ASSET
    with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as package:
        for path in sorted(args.dist.rglob('*')):
            if path.is_symlink():
                raise ValueError(f'Symlink in distribution: {path}')
            if path.is_file():
                package.write(path, path.relative_to(args.dist).as_posix())
    if archive.stat().st_size > 128 * 1024 * 1024:
        raise ValueError('Archive exceeds updater size limit')
    with zipfile.ZipFile(archive) as package:
        if len(package.infolist()) > 128 or sum(p.file_size for p in package.infolist()) > 256 * 1024 * 1024 or package.testzip() is not None:
            raise ValueError('Archive exceeds limits or failed CRC verification')
    with archive.open('rb') as stream:
        checksum = hashlib.file_digest(stream, 'sha256').hexdigest()
    (args.output / (ASSET + '.sha256')).write_text(f'{checksum}  {ASSET}\n', encoding='ascii')
    print(f'{ASSET}: {archive.stat().st_size} bytes; SHA256 {checksum}')


if __name__ == '__main__':
    main()
