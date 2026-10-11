"""Bounded STEP-to-IPT command; publish only after normal worker shutdown."""
from __future__ import annotations

import argparse
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def _worker(source, directory, report_path):
    from .step_parts import StepPartWriteError, write_step_parts
    try:
        result = write_step_parts(source, directory)
        report = dict(status='success', part_count=result.part_count,
                      source_sha256=result.source_sha256, native_validation='not_run')
    except Exception as error:
        report = dict(status='error', native_validation='not_run', error=str(error))
        if isinstance(error, StepPartWriteError):
            report['detail'] = error.report
    Path(report_path).write_text(json.dumps(report, ensure_ascii=False), encoding='utf-8')


def write_job(source: Path, destination: Path, timeout: float) -> dict:
    from .step_parts import _publish_directory
    report = dict(status='error', operation='write-step', native_validation='not_run',
                  source=str(source), output_directory=str(destination), published=False)
    try:
        if os.path.lexists(destination):
            raise FileExistsError(f'Output already exists: {destination}')
        with tempfile.TemporaryDirectory(prefix='.inventor-cli-write-', dir=destination.parent) as temporary:
            root = Path(temporary)
            result_path, staged = root/'result.json', root/'parts'
            with tempfile.TemporaryFile(mode='w+') as log:
                try:
                    child = subprocess.run(
                        [sys.executable, '-m', 'inventor_kit.writer_cli', '_worker',
                         str(source), str(staged), str(result_path)],
                        stdout=log, stderr=log, timeout=timeout, check=False)
                except subprocess.TimeoutExpired:
                    report['error'] = f'Input exceeded {timeout:g} seconds'
                    report['code'] = 'execution.timeout'
                    return report
            if child.returncode != 0:
                raise RuntimeError(f'Writer process exited with code {child.returncode}')
            if not result_path.is_file() or result_path.stat().st_size > 1024*1024:
                raise RuntimeError('Writer report missing or exceeds byte limit')
            result = json.loads(result_path.read_text(encoding='utf-8'))
            report.update(result)
            if report['status'] == 'success':
                _publish_directory(staged, destination)
                report.update(published=True, report_path=str(destination/'batch.json'))
    except Exception as error:
        report.update(status='error', error=str(error))
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description='Write each metric STEP solid occurrence as a new standalone IPT')
    parser.add_argument('source', type=Path)
    parser.add_argument('output_directory', type=Path, help='New directory; its parent must exist')
    parser.add_argument('--timeout', type=float, default=600, help='Total seconds, including native conversion (default: 600)')
    args = parser.parse_args(argv)
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error('--timeout must be finite and positive')
    result = write_job(args.source.absolute(), args.output_directory.absolute(), args.timeout)
    print(json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False))
    return 0 if result['status'] == 'success' else 1


if __name__ == '__main__':
    if len(sys.argv) == 5 and sys.argv[1] == '_worker':
        _worker(*sys.argv[2:])
    else:
        raise SystemExit(main())
