# Write native Inventor parts

English | [日本語](part-writer.ja.md)

The experimental writer creates standalone `.ipt` files without Inventor, the
Inventor API, or an existing IPT template. It accepts one solid per part and
stores exact geometry as a nonparametric base feature. Display meshes accompany
the geometry. Existing files and directories are never overwritten.

This API is part of the **0.7.0 release candidate** and requires `cq-acis>=0.3.10,<0.4`.
Install a candidate wheel from the [distribution workflow](https://github.com/monozukuri-ai/inventor-kit/actions/workflows/release.yml)
until inventor-kit 0.7.0 is published. Earlier published builds do not include this API.

## Create a part from CadQuery

```python
import cadquery as cq
import inventor_kit as ik

solid = cq.Workplane("XY").circle(10).extrude(20).val()
part = ik.new_part(name="Cylinder", title="Test part", part_number="CYL-001")
part.add_body(solid, name="Body", feature_name="Base feature")
report = part.saveas("cylinder.ipt")
print(report.path, report.report_path, report.native_validation)
```

`new_part` accepts `name`, `title`, `part_number`, `units="mm"` and an optional
`target_profile`. `add_body` accepts one valid CadQuery `Solid` in millimetres.
Names, Title and Part Number may contain Unicode and must fit in 255 UTF-16
units, without NUL or unpaired surrogates; document, body and feature names must
be nonempty. Inventor's displayed document name follows its filename.

`saveas` creates the IPT and `<path>.json`, recording its hash, metadata, bounds,
topology and qualification scope. Both paths must be new and the parent must
exist. The two files are written after encoding; a process interruption can
leave one of the pair. Invalid input raises `PartWriteError`.

## Convert a STEP assembly to separate IPTs

```python
import inventor_kit as ik

result = ik.write_step_parts("assembly.step", "output_parts")
print(result.part_count, result.report_path)  # output_parts/batch.json
```

The STEP must declare millimetres. Each solid occurrence becomes a standalone
IPT with its world placement applied once. Repeated definitions remain separate
files. XDE product names become body names, Title and Part Number. Names are
those decoded by OCCT, which can normalize surrounding STEP string whitespace.
Each filename uses a sequence number and a Windows-safe name; `batch.json`
retains original names, transforms and the file mapping.

The output directory must not exist; its parent must exist. All parts are
staged and checked for topology, volume, area, bounds and solid difference before
the directory is published exclusively. Existing empty directories, files and
symlinks are preserved. A failed batch raises `StepPartWriteError`; its `report`
identifies the phase and, during encoding, the occurrence. Unsupported geometry,
external documents, invalid placements, changed input and exceeded budgets
prevent publication of the batch.

The same operation is available as a CLI:

```sh
python -m inventor_kit write-step assembly.step output_parts --timeout 600
```

It prints a JSON result and exits with 0 on success or 1 on failure. The timeout
covers the whole conversion, including native geometry processing. The output
directory is published only after the worker exits normally. `batch.json` and
per-part reports describe successfully written files; a failed command reports
its error on stdout. For API calls, supervise a separate process when a wall-clock
or memory bound is required.

## Supported scope and limits

| Area | Supported scope |
| --- | --- |
| Geometry | One closed valid solid; planar/cylindrical faces; line/circle/ellipse edges |
| Units | Explicit millimetres |
| Document | One nonparametric base feature; fresh independent document identities |
| Metadata | Document label, body/feature names, Title and Part Number |
| STEP batch | Names, nested rigid placements, repeated solid occurrences |
| Outside scope | IAM/IDW output, multiple-body IPTs, feature-history reconstruction, imported appearance, general NURBS surfaces |

`StepPartLimits` defaults to 32 MiB of input, 250,000 STEP entities, 256 parts,
4,096 hierarchy nodes, depth 32, 32,768 total faces/edges/vertices and 256 MiB of
output. Each part is limited to 512 faces plus edges. Display arrays and total
mesh work are bounded separately. Unsupported input is rejected; geometry is
not replaced by a mesh or silently omitted.

Some otherwise valid analytic inputs can fail the round-trip comparison when
OCCT cannot produce a valid solid difference, including certain rotated parts
with holes. Such batches are rejected; matching volume and bounds alone do not
override that failure.

The profile is `inventor-ipt-rse31-metric-analytic-base-feature-v21`.
Independent native checks used Inventor 2027.1 build 311270010, covering 11
analytic parts and a separate 32-part assembly converted to individual IPTs.
The corpus passed visible open, update and save/reopen, with machining checks
on all 11 analytic cases and four representative assembly parts. Saved geometry,
metadata, placement and native STEP exports were compared independently.
This is bounded development-corpus evidence, not qualification of every accepted
input, release artifact or Inventor version.

Every newly generated file reports `native_validation="not_run"`; writing a
file does not launch Inventor. `ik.capabilities()["part_writer"]` reports the
scope and whether this extension exposes the writer. Distribution checks use
synthetic inputs, while native qualification remains a separate procedure.

Normal Python builds enable the `analytic-writer` Rust feature. Rust-only users
can disable default features for a reader build. Exclusive batch publication
requires `renameat2` on Linux, `renamex_np` on macOS, or directory rename on
Windows; unsupported filesystem operations raise an error.
