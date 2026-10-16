"""Portable flat IAM bundles from the bounded initial-state IPT writer profile."""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile

from .writer_profiles import ASSEMBLY_PROFILES, resolve_target

ASSEMBLY_PROFILE = ASSEMBLY_PROFILES[2027]
ASSEMBLY_PROFILE_2026 = ASSEMBLY_PROFILES[2026]
MAX_PART_BYTES = 64 * 1024 * 1024
MAX_BUNDLE_BYTES = 256 * 1024 * 1024


class AssemblyWriteError(ValueError):
    """Unsupported input or assembly specification; no bundle was published."""


@dataclass(frozen=True)
class AssemblyWriteReport:
    directory: Path
    path: Path
    report_path: Path
    definition_count: int
    occurrence_count: int
    sha256: str
    target_profile: str = ASSEMBLY_PROFILE
    native_validation: str = 'not_run'


def _name(value):
    if (not isinstance(value, str) or not value
            or any(0xD800 <= ord(c) <= 0xDFFF for c in value)
            or len(value.encode('utf-16-le')) > 510
            or any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in value)):
        raise AssemblyWriteError('Names require 1..255 UTF-16 units without control characters')
    return value


def _filename(value, suffix):
    _name(value)
    stem = value.split('.')[0].upper()
    if (len(value.encode('utf-16-le')) > 480 or not value.lower().endswith(suffix)
            or any(c in value for c in '/\\:<>"|?*') or value.endswith((' ', '.'))
            or stem in ('CON', 'PRN', 'AUX', 'NUL')
            or (stem[:3] in ('COM', 'LPT') and stem[3:] in ('1', '2', '3', '4', '5', '6', '7', '8', '9', '¹', '²', '³'))):
        raise AssemblyWriteError(f'Expected a portable same-folder {suffix} filename')
    return value


def _artifact(name, data):
    return dict(path=name, bytes=len(data), sha256=hashlib.sha256(data).hexdigest())


class AssemblyBuilder:
    """Build a flat fixed assembly without Inventor or COM.

    Parts are copied from snapshots captured by :meth:`add_part`. Each occurrence
    is visible, grounded and has the identity transform. The initial profile
    accepts newly authored single-solid IPTs; arbitrary or native-resaved IPTs,
    constraints, nested IAMs and nonidentity transforms are unsupported.
    """

    def __init__(self, *, name: str, part_number: str | None = None, target_year: int = 2027):
        from . import _inventor
        if not hasattr(_inventor, 'encode_fixed_assembly'):
            raise AssemblyWriteError('This build does not include the IAM writer')
        try:
            self._target_year, self._target_profile = resolve_target(ASSEMBLY_PROFILES, target_year)
        except ValueError as error:
            raise AssemblyWriteError(str(error)) from error
        self._name = _name(name)
        self._part_number = _name(name if part_number is None else part_number)
        self._parts = []
        self._occurrences = []

    def add_part(self, path: str | os.PathLike, *, name: str | None = None,
                 filename: str | None = None) -> AssemblyBuilder:
        """Snapshot an IPT and add one placement; repeated filenames reuse a part.

        ``filename`` selects the bundle filename, not an external reference path.
        The source may be deleted after binding; its captured bytes are retained.
        """
        from . import _inventor
        if len(self._occurrences) >= 32:
            raise AssemblyWriteError('The fixed IAM profile supports at most 32 occurrences')
        path = Path(path)
        filename = _filename(path.name if filename is None else filename, '.ipt')
        info = path.stat()
        if (path.suffix.lower() != '.ipt' or not stat.S_ISREG(info.st_mode)
                or info.st_size > MAX_PART_BYTES):
            raise AssemblyWriteError('Expected a regular IPT file of at most 64 MiB')
        with path.open('rb') as source:
            data = source.read(MAX_PART_BYTES + 1)
        if not data or len(data) > MAX_PART_BYTES:
            raise AssemblyWriteError('IPT byte budget exceeded')
        existing = next((i for i, p in enumerate(self._parts)
                         if p[0].casefold() == filename.casefold()), None)
        if existing is not None:
            old_name, old_data, metadata = self._parts[existing]
            if old_name != filename or old_data != data:
                raise AssemblyWriteError('Conflicting Windows filenames or changed part bytes')
            index = existing
        else:
            if sum(len(p[1]) for p in self._parts) + len(data) > MAX_BUNDLE_BYTES:
                raise AssemblyWriteError('Aggregate IPT byte budget exceeded')
            try:
                metadata = json.loads(_inventor.inspect_authored_part(data, filename))
            except ValueError as error:
                raise AssemblyWriteError(str(error)) from error
            for _, old_data, old in self._parts:
                if old['document_id'] == metadata['document_id'] and old_data != data:
                    raise AssemblyWriteError('Conflicting IPT bytes with the same document identity')
            index = len(self._parts)
        if metadata['target_year'] != self._target_year:
            raise AssemblyWriteError('IAM and IPT target versions must match')
        count = sum(o['part'] == index for o in self._occurrences) + 1
        occurrence_name = _name(name if name is not None else f"{metadata['display_name']}:{count}")
        if occurrence_name.casefold() in {o['name'].casefold() for o in self._occurrences}:
            raise AssemblyWriteError('Occurrence names must be unique ignoring case')
        if existing is None:
            self._parts.append((filename, data, metadata))
        self._occurrences.append(dict(part=index, name=occurrence_name))
        return self

    def save_bundle(self, directory: str | os.PathLike, *, filename: str = 'assembly.iam') -> AssemblyWriteReport:
        """Publish IAM, unchanged IPT snapshots and bundle.json in a new directory.

        All bytes are staged before an exclusive directory rename. Existing paths,
        including empty directories and symlinks, are never overwritten. The
        report's ``native_validation`` is always ``not_run`` for this output.
        """
        from . import _inventor
        from .step_parts import _publish_directory
        filename = _filename(filename, '.iam')
        target = Path(directory).absolute()
        if os.path.lexists(target):
            raise FileExistsError(target)
        if not self._occurrences:
            raise AssemblyWriteError('At least one part occurrence is required')
        spec = dict(target_year=self._target_year, name=self._name, part_number=self._part_number, occurrences=self._occurrences)
        try:
            data, raw = _inventor.encode_fixed_assembly(
                json.dumps(spec, ensure_ascii=False, allow_nan=False),
                [(name, data) for name, data, _ in self._parts])
        except ValueError as error:
            raise AssemblyWriteError(str(error)) from error
        identity = _artifact(filename, data)
        report = dict(schema_version=1, operation='write-assembly', status='success',
                      target_profile=self._target_profile, target_year=self._target_year, units='mm', native_validation='not_run',
                      assembly=identity, document=json.loads(raw),
                      parts=[_artifact(name, data) for name, data, _ in self._parts])
        report_data = (json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + '\n').encode('utf-8')
        if sum(len(p[1]) for p in self._parts) + len(data) + len(report_data) > MAX_BUNDLE_BYTES:
            raise AssemblyWriteError('Aggregate bundle byte budget exceeded')
        with tempfile.TemporaryDirectory(prefix='.inventor-assembly-', dir=target.parent) as temporary:
            staging = Path(temporary) / 'bundle'
            staging.mkdir()
            (staging / filename).write_bytes(data)
            for name, content, _ in self._parts:
                (staging / name).write_bytes(content)
            (staging / 'bundle.json').write_bytes(report_data)
            _publish_directory(staging, target)
        return AssemblyWriteReport(target, target / filename, target / 'bundle.json',
                                   len(self._parts), len(self._occurrences), identity['sha256'], self._target_profile)


def new_assembly(*, name: str, part_number: str | None = None, target_year: int = 2027) -> AssemblyBuilder:
    return AssemblyBuilder(name=name, part_number=part_number, target_year=target_year)
