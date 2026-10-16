"""Experimental source-free single-solid IPT authoring.

The analytic writer feature and cq-acis semantic authoring API are required.
Inventor is only used in separate qualification, never by this API.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import tempfile

from .writer_profiles import PART_PROFILES, resolve_target

ANALYTIC_PROFILE = PART_PROFILES[2027]
ANALYTIC_PROFILE_2026 = PART_PROFILES[2026]


class PartWriteError(ValueError):
    """The requested document is outside the bounded writer contract."""


@dataclass(frozen=True)
class PartWriteReport:
    path: Path
    report_path: Path
    sha256: str
    bytes: int
    target_profile: str
    native_validation: str = 'not_run'


def _name(value: str, *, empty: bool = False) -> str:
    if not isinstance(value, str) or (not empty and not value) or '\0' in value:
        raise PartWriteError('names must be text without NUL; document/body/feature names must be nonempty')
    try:
        count = len(value.encode('utf-16-le')) // 2
    except UnicodeEncodeError as exc:
        raise PartWriteError('unpaired surrogate in metadata') from exc
    if count > 255:
        raise PartWriteError('metadata exceeds 255 UTF-16 code units')
    return value


def _bounds(shape):
    from OCP.Bnd import Bnd_Box
    from OCP.BRepBndLib import BRepBndLib
    box = Bnd_Box()
    BRepBndLib.AddOptimal_s(shape.wrapped, box, False, False)
    return list(box.Get())


def _display(shape):
    faces, edges = shape.Faces(), shape.Edges()
    if len(faces) + len(edges) > 512:
        raise PartWriteError('display topology exceeds 512 faces plus edges')
    result = dict(bounds_mm=_bounds(shape), faces=[], edges=[])
    budget = 0
    for face in faces:
        vertices, triangles = face.tessellate(0.01, 0.1)
        budget += len(vertices) + len(triangles)
        if not vertices or not triangles or max(len(vertices), len(triangles)) > 200000 or budget > 1000000:
            raise PartWriteError('display mesh budget exceeded')
        result['faces'].append(dict(bounds_mm=_bounds(face), vertices_mm=[p.toTuple() for p in vertices],
            triangles=triangles, normals=[face.normalAt(p).toTuple() for p in vertices],
            uv=[face.paramAt(p) for p in vertices]))
    for edge in edges:
        points, _ = edge.sample(0.01)
        if edge.IsClosed() and (points[-1] - points[0]).Length > 1e-10:
            points.append(points[0])
        budget += len(points)
        if not 2 <= len(points) <= 200000 or budget > 1000000:
            raise PartWriteError('display polyline budget exceeded')
        result['edges'].append(dict(bounds_mm=_bounds(edge), points_mm=[p.toTuple() for p in points]))
    return result


class PartBuilder:
    """One new metric part with one exact analytic solid and one base feature.

    Supporting surfaces are planes/cylinders; edges are lines/circles/ellipses.
    New native files have no source-document references. Meshes are display data.
    Save creates a new IPT and a JSON report and refuses either existing path.
    ``name`` is an authored document label; Inventor DisplayName follows the
    saved filename. Title and Part Number are separate iProperties.
    """
    def __init__(self, *, name: str, units: str = 'mm', target_profile: str | None = None, target_year: int | None = None,
                 title: str = '', part_number: str = ''):
        if units != 'mm':
            raise PartWriteError('only explicit millimetre geometry is supported')
        try:
            self._target_year, self._target_profile = resolve_target(PART_PROFILES, target_year, target_profile)
        except ValueError as error:
            raise PartWriteError(str(error)) from error
        self._metadata = dict(document_name=_name(name), title=_name(title, empty=True),
                              part_number=_name(part_number, empty=True))
        self._body_json: str | None = None

    def add_body(self, shape, *, name: str = 'Solid1', feature_name: str = 'Base feature') -> PartBuilder:
        if self._body_json is not None:
            raise PartWriteError('exactly one body is supported')
        name, feature_name = _name(name), _name(feature_name)
        from . import _inventor
        if not hasattr(_inventor, 'encode_analytic_part'):
            raise PartWriteError('this native extension was built without the analytic-writer feature')
        try:
            import cadquery as cq
            from cq_acis.authoring import from_cadquery
            if not isinstance(shape, cq.Solid) or not shape.isValid() or len(shape.Solids()) != 1:
                raise PartWriteError('one valid CadQuery Solid is required')
            # Detach geometry and triangulation from caller-owned objects.
            from OCP.BRepBuilderAPI import BRepBuilderAPI_Copy
            shape = cq.Solid(BRepBuilderAPI_Copy(shape.wrapped, True, False).Shape())
            authored = from_cadquery(shape)
            if not hasattr(authored, 'to_spec'):
                raise PartWriteError('cq-acis semantic authoring specification is unavailable in this build')
            body = dict(solid=authored.to_spec(), display=_display(shape), name=name, feature_name=feature_name)
            encoded = json.dumps(body, allow_nan=False, separators=(',', ':'))
            if len(encoded.encode('utf-8')) > 60 * 1024 * 1024:
                raise PartWriteError('semantic part byte budget exceeded')
            self._body_json = encoded
        except (ValueError, TypeError) as exc:
            if isinstance(exc, PartWriteError):
                raise
            raise PartWriteError(str(exc)) from exc
        return self

    def saveas(self, path: str | os.PathLike) -> PartWriteReport:
        """Create an IPT plus ``<path>.json`` without replacing either file.

        The two files are published with exclusive links after full encoding.
        An interrupted process may leave one file; existing files are never
        treated as successful output or silently overwritten on retry.
        """
        if self._body_json is None:
            raise PartWriteError('add one body before saving')
        path = Path(path).absolute()
        report_path = Path(str(path) + '.json')
        if path.suffix.lower() != '.ipt':
            raise PartWriteError('new part path must end with .ipt')
        for target in (path, report_path):
            if os.path.lexists(target):
                raise FileExistsError(target)
        body = json.loads(self._body_json)
        spec = dict(profile=self._target_profile, solid=body['solid'], display=body['display'],
                    metadata={**self._metadata, 'body_name': body['name'], 'feature_name': body['feature_name']})
        from . import _inventor
        try:
            data = _inventor.encode_analytic_part(json.dumps(spec, allow_nan=False, separators=(',', ':')))
        except ValueError as exc:
            raise PartWriteError(str(exc)) from exc
        digest = hashlib.sha256(data).hexdigest()
        report = dict(schema_version=1, target_profile=self._target_profile, target_year=self._target_year, native_validation='not_run',
            profile_qualification='native-qualified-bounded-corpus',
            profile_qualification_scope=dict(inventor_build='311270010', generated_cases=11,
                controls=2, evidence='M4-V21', native_machining_save_reopen=True,
                development_build_only=True),
            product_inventor_api_dependency=False, source_IPT_template_inputs=0, units='mm', bodies=1,
            artifact=dict(path=str(path), bytes=len(data), sha256=digest), metadata=spec['metadata'],
            bounds_mm=body['display']['bounds_mm'],
            topology=dict(faces=len(body['solid']['faces']), edges=len(body['solid']['edges']),
                          vertices=len(body['solid']['vertices_mm'])))
        if self._target_year == 2026:
            report['profile_qualification'] = 'native-qualified-bounded-corpus'
            report['profile_qualification_scope'] = dict(inventor_build='304435000',
                generated_cases=32, controls=1, evidence='Inventor-2026-Drawing-v2',
                native_machining_save_reopen=True, development_build_only=True,
                machining_evidence_scope='prior-constructor-corpus',
                integrated_build_native_validation='qualified-bounded-corpus',
                integrated_build_evidence='Inventor-2026-Public-v1',
                integrated_build_scope='32 parts in a flat IAM: open/save/reopen and moved-bundle resolution/resave/reopen; exact result SAB parity with the prior machining corpus')
        with tempfile.TemporaryDirectory(prefix='.inventor-write-', dir=path.parent) as staging:
            ipt, sidecar = Path(staging)/'part', Path(staging)/'report'
            for target, payload in ((ipt, data), (sidecar, (json.dumps(report, ensure_ascii=False, indent=2)+'\n').encode('utf-8'))):
                with target.open('xb') as stream:
                    stream.write(payload)
                    stream.flush()
                    os.fsync(stream.fileno())
            os.link(ipt, path)
            try:
                os.link(sidecar, report_path)
            except BaseException:
                # Only roll back the inode this call created.
                try:
                    current, owned = path.stat(follow_symlinks=False), ipt.stat()
                    if (current.st_dev, current.st_ino) == (owned.st_dev, owned.st_ino):
                        path.unlink()
                except FileNotFoundError:
                    pass
                raise
        return PartWriteReport(path, report_path, digest, len(data), self._target_profile)


def new_part(*, name: str, units: str = 'mm', target_profile: str | None = None, target_year: int | None = None,
             title: str = '', part_number: str = '') -> PartBuilder:
    """Start a new independent metric part; see :class:`PartBuilder`."""
    return PartBuilder(name=name, units=units, target_profile=target_profile, target_year=target_year, title=title, part_number=part_number)
