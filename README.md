# inventor-kit

English | [日本語](README.ja.md)

Read Autodesk Inventor document metadata and saved part geometry with a Rust
parser and a Python API. Supported geometry can be converted to
`cq_acis.AcisModel` and CadQuery objects. Runtime use does not require network
access, Inventor, Windows, or user-provided schemas.

```text
IPT → CFB / RSe → saved SAB / ASM → acis-core → AcisModel → CadQuery
IAM → saved references and placements → local part resolution → CadQuery Assembly
```

The parser supports a limited set of validated format profiles. It does not
determine the current Model State, reevaluate feature history, or approximate
unsupported geometry.

## Installation

Requires Python 3.11 or later, `cq-acis>=0.3.2,<0.4`, and shared model API 2.
Where compatible published wheels are available, install with:

```sh
python -m pip install inventor-kit
```

Source builds use Rust 1.93, maturin, and `acis-core` / `acis-py-bridge` 0.3.2
from crates.io. See the [development guide](docs/development.md) and
[release requirements](docs/releasing.md).

## Reading parts

```python
import inventor_kit as ik
import cadquery as cq

doc = ik.read_file("part.ipt")
print(doc.summary["status"], doc.summary["diagnostics"])
if doc.model is not None:
    shape = doc.to_cadquery()  # Raises for unsupported surfaces, trims, etc.
    cq.exporters.export(shape, "part.step")
```

For metadata alone, use `inspect_file`, which does not import the Python geometry
modules.

```python
info = ik.inspect_file("part.ipt").metadata
for prop in info.find_properties(semantic_name="part_number"):
    print(prop.value, prop.source.stream, prop.state_binding)
```

```sh
python -m inventor_kit part.ipt
python -m inventor_kit part.ipt --step part.step
python -m inventor_kit drawing.idw --metadata-only
python -m inventor_kit part.ipt --list-candidates
```

For local visual inspection of parts and saved assemblies, install the optional [viewer](docs/viewer.md).

```sh
python -m pip install 'inventor-kit[viewer]'
python -m inventor_kit.viewer part.ipt
```

![Inventor Kit viewer displaying the NIST CTC 03 model](https://raw.githubusercontent.com/monozukuri-ai/inventor-kit/main/assets/viewer.png)

Model: [NIST CTC 03](https://www.nist.gov/ctl/smart-connected-systems-division/smart-connected-manufacturing-systems-group/mbe-pmi-0) (saved geometry).

## Reading assemblies

```python
assembly = ik.read_assembly_file("assembly.iam", search_roots=["parts"])
converted = assembly.to_cadquery(allow_unverified_state=True, allow_partial=True)
print(converted.omissions, converted.reference_issues)
report = converted.export_step("assembly.step", allow_partial=True)
```

Using saved placements requires explicit opt-in. Missing parts and unsupported
geometry remain recorded in the result. Because the current state is unverified,
`complete` is `False`. Assembly STEP export refuses to overwrite existing files
and writes provenance and omission details to an accompanying JSON report.

## Limits and supported scope

```python
limits = ik.Limits(max_file_bytes=32 * 1024 * 1024, max_candidates=16)
doc = ik.read_file("part.ipt", limits=limits)
print(ik.capabilities())
```

Limits can be lowered per document. They do not guarantee bounds on process
memory, runtime, or OCCT computation. See the [API guide](docs/api.md) for defaults,
diagnostics, and saved candidate selection.

| Guide | Contents |
| --- | --- |
| [Supported scope](docs/support.md) | Format profiles and geometry, state, and assembly limitations |
| [API](docs/api.md) | Metadata, saved candidates, units, reference resolution, and limits |
| [Validation](docs/validation.md) | Regression and holdout checks, comparison evidence, and completed validation |
| [Development](docs/development.md) | Builds, tests, fuzzing, and public directory roles |
| [Releasing](docs/releasing.md) | Wheels, sdists, CI, and publication requirements |
| [Format references](docs/format-reference.md) | Implementation references and licenses |

Licensed under MIT. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for
third-party notices. Public samples are validated against pinned hashes and
sources; the CAD files themselves are excluded from distribution packages.
