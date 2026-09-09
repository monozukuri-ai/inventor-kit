"""Bounded ASan/libFuzzer smoke runs. A smoke pass is not exhaustive safety proof."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--seconds', type=int, default=30)
    parser.add_argument('--toolchain', default='nightly-2026-02-02')
    parser.add_argument('--output', type=Path, default=ROOT / 'internal/reports/latest/fuzz.json')
    args = parser.parse_args()
    if not 1 <= args.seconds <= 600:
        parser.error('--seconds must be 1..600 per target')
    lock = ROOT / 'fuzz/Cargo.lock'
    before = hashlib.sha256(lock.read_bytes()).hexdigest()
    env = {**os.environ, 'CARGO_NET_OFFLINE': 'true'}
    subprocess.run(['cargo', 'metadata', '--manifest-path', str(ROOT/'fuzz/Cargo.toml'),
                    '--locked', '--offline', '--format-version', '1'],
                   cwd=ROOT.parent, env=env, check=True, stdout=subprocess.DEVNULL)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    targets = []
    for name in ('container', 'streams'):
        corpus = ROOT / 'fuzz/corpus' / name
        if not corpus.is_dir() or not any(corpus.iterdir()):
            raise ValueError('Missing fuzz seeds; run scripts/seed_fuzz_corpus.py')
        log = args.output.parent / (args.output.stem + '-' + name + '-fuzz.log')
        command = ['cargo', '+' + args.toolchain, 'fuzz', 'run', name, str(corpus),
                   '--fuzz-dir', str(ROOT/'fuzz'), '--', '-max_total_time='+str(args.seconds),
                   '-timeout=5', '-rss_limit_mb=1024', '-max_len=2097152', '-seed=20260910']
        with log.open('w') as output:
            try:
                code = subprocess.run(command, cwd=ROOT.parent, env=env, stdout=output,
                                      stderr=subprocess.STDOUT, timeout=args.seconds+180).returncode
            except subprocess.TimeoutExpired:
                code = -1
        text = log.read_text(errors='replace')
        status = 'passed' if code == 0 and 'Done ' in text else 'failed'
        targets.append(dict(target=name, status=status, exit_code=code, log=str(log), seconds=args.seconds,
                            timeout_seconds=5, rss_limit_mb=1024, sanitizer='address', seed=20260910))
    unchanged = before == hashlib.sha256(lock.read_bytes()).hexdigest()
    passed = unchanged and all(r['status'] == 'passed' for r in targets)
    report = dict(schema_version=1, status='passed' if passed else 'failed', targets=targets,
                  toolchain=subprocess.check_output(['rustc', '+'+args.toolchain, '--version'], text=True).strip(),
                  lock_sha256=before, lock_unchanged=unchanged, holdouts_used=False,
                  scope='time-bounded mutation smoke; filesystem resolver cycles covered by Rust tests')
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    raise SystemExit(0 if passed else 1)


if __name__ == '__main__':
    main()
