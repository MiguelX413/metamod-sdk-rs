"""Versioned provenance for synthetic native runs (integrity, not attestation)."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import uuid

from artifacts import digest

PREFIX = 'MM_AUDIT_BUILD '
ARTIFACTS = {'loader', 'hello-world', 'engine-demo', 'metamod-audit'}


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def source_identity():
    revisions = {}
    for name in ('.', 'upstream/metamod-hl1', 'upstream/hlsdk'):
        revisions[name] = {
            'commit': command('git', '-C', name, 'rev-parse', 'HEAD'),
            'status': command('git', '-C', name, 'status', '--porcelain', '--untracked-files=normal'),
            'diff_sha256': hashlib.sha256(subprocess.check_output(
                ['git', '-C', name, 'diff', 'HEAD', '--binary'])).hexdigest(),
        }
    return revisions


def capture(output, work):
    output, work = Path(output), Path(work)
    artifacts = output / 'artifacts'
    artifacts.mkdir()
    paths = {'loader': work / 'load', **{
        name: work / 'target/i686-unknown-linux-gnu/release' / ('lib' + name.replace('-', '_') + '.so')
        for name in ARTIFACTS - {'loader'}}}
    hashes = {}
    for name, source in sorted(paths.items()):
        shutil.copy2(source, artifacts / name)
        hashes[name] = digest(artifacts / name)
    revisions = source_identity()
    if revisions != json.loads((output / 'source-before.json').read_text()):
        raise ValueError('source state changed during native build')
    combined = os.environ.get('METAMOD_SANITIZE_PLUGIN') == '1'
    record = {
        'schema_version': 1, 'run_id': os.environ['METAMOD_NATIVE_RUN_ID'],
        'target': 'i686-unknown-linux-gnu', 'artifacts': hashes, 'revisions': revisions,
        'toolchain': {'rust': command('rustc', *(['+nightly'] if combined else []), '-vV'),
                      'cxx': command(os.environ.get('CXX', 'c++'), '--version')},
        'sanitizers': {'rust_asan': combined,
                       'loader_asan_ubsan_lsan': os.environ.get('METAMOD_SANITIZE_LOADER') == '1'},
        'environment': {key: os.environ.get(key, '') for key in
                        ('RUSTFLAGS', 'ASAN_OPTIONS', 'UBSAN_OPTIONS', 'LSAN_OPTIONS')},
        'effective_options': {
            'rustflags': (os.environ.get('RUSTFLAGS', '') +
                          (' -Zsanitizer=address -Zexternal-clangrt -Clinker=' +
                           os.environ.get('METAMOD_SANITIZER_CLANG', 'clang') if combined else '')).strip(),
            'asan_options': (os.environ.get('ASAN_OPTIONS', '') + ':detect_leaks=1').lstrip(':')
                            if os.environ.get('METAMOD_SANITIZE_LOADER') == '1' else os.environ.get('ASAN_OPTIONS', ''),
        },
    }
    (output / 'build.json').write_text(json.dumps(record, indent=2) + '\n')
    print(PREFIX + json.dumps(record, sort_keys=True), flush=True)


def validate(runtime, logs, base):
    """Return a verified manifest, raising on incomplete or mismatched evidence."""
    binding = runtime['native_identity']
    build_path = base / 'build.json'
    if binding['build_sha256'] != digest(build_path):
        raise ValueError('native build manifest hash mismatch')
    build = json.loads(build_path.read_text())
    if (build['schema_version'] != 1 or build['run_id'] != binding['run_id']
            or build['target'] != 'i686-unknown-linux-gnu'
            or runtime.get('os') != 'linux' or runtime.get('architecture') != 'i686'):
        raise ValueError('native run or target mismatch')
    uuid.UUID(build['run_id'])
    if set(build['artifacts']) != ARTIFACTS:
        raise ValueError('incomplete native artifacts')
    for name, expected in build['artifacts'].items():
        if digest(base / 'artifacts' / name) != expected:
            raise ValueError('native artifact hash mismatch: ' + name)
    if set(binding['logs']) != {path.name for path in logs} or len(logs) != len(binding['logs']):
        raise ValueError('native log set mismatch')
    markers = []
    for path in logs:
        if digest(path) != binding['logs'][path.name]:
            raise ValueError('native log hash mismatch')
        markers.extend(json.loads(line[len(PREFIX):]) for line in path.read_bytes().splitlines()
                       if line.startswith(PREFIX.encode()))
    if markers != [build]:
        raise ValueError('native transcript build identity mismatch')
    if (set(build['revisions']) != {'.', 'upstream/metamod-hl1', 'upstream/hlsdk'}
            or not all(build['toolchain'].get(key) for key in ('rust', 'cxx'))
            or set(build['sanitizers']) != {'rust_asan', 'loader_asan_ubsan_lsan'}):
        raise ValueError('incomplete native build metadata')
    if (not all(type(value) is bool for value in build['sanitizers'].values())
            or (build['sanitizers']['rust_asan'] and not build['sanitizers']['loader_asan_ubsan_lsan'])
            or not isinstance(build['environment'], dict)):
        raise ValueError('invalid native sanitizer metadata')
    for revision in build['revisions'].values():
        if (len(revision['commit']) != 40 or len(revision['diff_sha256']) != 64
                or not isinstance(revision['status'], str)):
            raise ValueError('invalid native revision')
    return build


if __name__ == '__main__':
    if sys.argv[1] == '--source':
        Path(sys.argv[2]).write_text(json.dumps(source_identity(), sort_keys=True) + '\n')
    else:
        capture(*sys.argv[1:])
