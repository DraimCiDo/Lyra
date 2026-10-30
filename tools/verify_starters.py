"""Developer/CI acceptance of trusted built-in starters. Never executes model-generated plans."""
import argparse
import json
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile


def run(command, cwd=None):
    result = subprocess.run(command, cwd=cwd, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True, encoding='utf-8', errors='replace', timeout=180)
    if result.returncode:
        print(result.stdout[-16000:])
        raise RuntimeError(f'Acceptance command failed ({result.returncode}): {command[0]}')
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--minecraft', action='store_true')
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    profiles = ['python', 'typescript']
    if args.minecraft:
        profiles += ['paper-java', 'paper-kotlin', 'spigot-java', 'spigot-kotlin']
    with tempfile.TemporaryDirectory(prefix='lyra-starters-') as temporary:
        parent = Path(temporary) / 'Проекты с пробелами'
        parent.mkdir()
        for profile in profiles:
            name = 'accept-' + profile
            command = [binary, 'project', 'starter', '--name', name, '--profile', profile]
            if profile.startswith(('paper-', 'spigot-')):
                command += ['--target-version', '1.21.11']
            plan = run(command)
            parsed = json.loads(plan)
            assert not parsed['questions']
            plan_file = Path(temporary) / (profile + '.json')
            plan_file.write_text(plan, encoding='utf-8')
            info = json.loads(run([binary, 'project', 'create', '--plan', str(plan_file), '--parent', str(parent)]))
            assert info['compiled'] is False  # Evidence comes from this harness, not the model/application.
            root = parent / name
            if profile == 'python':
                python = sys.executable
                run([python, '-m', 'unittest', 'discover', '-s', 'tests'], cwd=root)
            elif profile == 'typescript':
                node = shutil.which('node')
                assert node, 'Node.js 22.18+ or 24 is required for acceptance'
                run([node, '--test', 'tests/greeting.test.ts'], cwd=root)
            else:
                maven = shutil.which('mvn')
                assert maven, 'Maven and a full JDK 21 are required for Minecraft acceptance'
                run([maven, '--batch-mode', 'verify'], cwd=root)
                jar = root / 'target' / (name + '-0.1.0.jar')
                with zipfile.ZipFile(jar) as package:
                    entries = package.namelist()
                    assert 'plugin.yml' in entries
                    major = struct.unpack_from('>H', package.read('local/generated/Main.class'), 6)[0]
                    assert major == 65, 'The plugin must target Java 21'
                    assert not any(n.startswith('org/bukkit/') for n in entries), 'Server API must not be bundled'
                    kotlin = profile.endswith('kotlin')
                    assert ('kotlin/jvm/internal/Intrinsics.class' in entries) == kotlin
                    if kotlin:
                        assert 'META-INF/licenses/kotlin-APACHE-2.0.txt' in entries
            print(f'{profile}: generated, reviewed recipe commands completed; game/server acceptance NOT run', flush=True)


if __name__ == '__main__':
    main()
