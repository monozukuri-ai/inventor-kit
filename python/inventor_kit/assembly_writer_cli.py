"""Isolated flat IAM bundle command; publish only after successful worker exit."""
from __future__ import annotations

import argparse
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def _worker(request_path, result_path):
    from .assembly_writer import new_assembly
    try:
        request = json.loads(Path(request_path).read_text(encoding='utf-8'))
        assembly = new_assembly(name=request['name'], part_number=request['part_number'])
        for part in request['parts']:
            assembly.add_part(part)
        result = assembly.save_bundle(request['staging'], filename=request['filename'])
        report = dict(status='success', definition_count=result.definition_count,
                      occurrence_count=result.occurrence_count, sha256=result.sha256)
    except Exception as error:
        report = dict(status='error', error=str(error))
    Path(result_path).write_text(json.dumps(report, ensure_ascii=False), encoding='utf-8')


def write_job(parts, destination, *, name='Assembly', part_number=None, filename='assembly.iam', timeout=120):
    from .step_parts import _publish_directory
    report = dict(status='error', operation='write-assembly', published=False,
                  output_directory=str(destination), native_validation='not_run')
    try:
        if not math.isfinite(timeout) or timeout <= 0:
            raise ValueError('timeout must be finite and positive')
        destination = Path(destination).absolute()
        if os.path.lexists(destination):
            raise FileExistsError(destination)
        if not 1 <= len(parts) <= 32:
            raise ValueError('Expected 1..32 IPT part occurrences')
        with tempfile.TemporaryDirectory(prefix='.inventor-assembly-cli-', dir=destination.parent) as temporary:
            root = Path(temporary)
            request_path, result_path, staging = root/'request.json', root/'result.json', root/'bundle'
            request_path.write_text(json.dumps(dict(parts=[str(Path(p).absolute()) for p in parts],
                staging=str(staging), name=name, part_number=part_number, filename=filename)), encoding='utf-8')
            with tempfile.TemporaryFile(mode='w+') as log:
                try:
                    child = subprocess.run([sys.executable, '-m', 'inventor_kit.assembly_writer_cli',
                        '_worker', str(request_path), str(result_path)], stdout=log, stderr=log,
                        timeout=timeout, check=False)
                except subprocess.TimeoutExpired:
                    report.update(error=f'Input exceeded {timeout:g} seconds', code='execution.timeout')
                    return report
            if child.returncode != 0:
                raise RuntimeError(f'Assembly writer exited with code {child.returncode}')
            if not result_path.is_file() or result_path.stat().st_size > 1024*1024:
                raise RuntimeError('Assembly worker report missing or oversized')
            report.update(json.loads(result_path.read_text(encoding='utf-8')))
            if report['status'] == 'success':
                _publish_directory(staging, destination)
                report.update(published=True, report_path=str(destination/'bundle.json'))
    except Exception as error:
        report.update(status='error', error=str(error))
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description='Bundle 1..32 newly authored IPTs into a flat fixed IAM')
    parser.add_argument('output_directory', type=Path, help='New directory; parent must exist')
    parser.add_argument('parts', nargs='+', type=Path, help='IPT occurrences in order; repeats are allowed')
    parser.add_argument('--name', default='Assembly')
    parser.add_argument('--part-number')
    parser.add_argument('--filename', default='assembly.iam')
    parser.add_argument('--timeout', type=float, default=120)
    args = parser.parse_args(argv)
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error('--timeout must be finite and positive')
    result = write_job(args.parts, args.output_directory, name=args.name, part_number=args.part_number,
                       filename=args.filename, timeout=args.timeout)
    print(json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False))
    return 0 if result['status'] == 'success' else 1


if __name__ == '__main__':
    if len(sys.argv) == 4 and sys.argv[1] == '_worker':
        _worker(*sys.argv[2:])
    else:
        raise SystemExit(main())
