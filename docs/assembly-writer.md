# Assembly writer

English | [日本語](assembly-writer.ja.md)

The experimental IAM writer bundles **1–32 newly authored IPT occurrences** into
a flat assembly without Inventor, COM, an existing IAM, or a native template.
The bounded public profile passed Inventor 2027.1 (build `311270010`) qualification
on 2026-10-12. These APIs are in the working tree and are not yet released.

## Python

```python
import inventor_kit as ik

assembly = ik.new_assembly(name="Frame", part_number="ASM-001")
assembly.add_part("parts/rail.ipt", name="Rail:1")
assembly.add_part("parts/plate.ipt", name="Plate:1")
result = assembly.save_bundle("frame_bundle", filename="frame.iam")
print(result.path, result.definition_count, result.occurrence_count)
```

Use IPTs produced by `new_part(...).saveas(...)` or `write_step_parts(...)` in the
bounded single-solid writer profile. The writer reads identity, display name and
bounds from the IPT itself. JSON sidecars do not establish a part's identity.
Arbitrary native IPTs and IPTs saved again in Inventor are outside this input profile.

Every placement is visible and grounded, with an identity transform. Position the
solid before creating its IPT. STEP-to-IPT output already contains world-coordinate
geometry, so no second placement transform is applied. Repeating a part creates
another occurrence in the same position and shares its file reference. Occurrence
names must be unique ignoring case. A default name uses the part's display name
and a per-definition occurrence number; specify `name` when different parts share
the same display name.

`add_part` captures a byte snapshot. Subsequent changes to the original path do not
change the bundle. `filename="rail.ipt"` can select a different portable basename.
Conflicting files with the same Windows filename or document identity are rejected.

`save_bundle` creates a new directory containing the IAM, unchanged referenced IPT
snapshots, and `bundle.json`. It validates and stages the complete bundle before an
exclusive directory rename. Its parent must exist. Existing files, directories and
symlinks are never replaced. Keep the entire directory together when moving it.

The report lists the exact filenames, byte counts, SHA-256 hashes, identities and
placements. `native_validation="not_run"` describes this output: generating a file
does not start Inventor. Repeated saves allocate fresh assembly identities.

## CLI

```sh
python -m inventor_kit write-assembly frame_bundle parts/rail.ipt parts/plate.ipt \
  --name Frame --part-number ASM-001 --filename frame.iam --timeout 120
```

Arguments after the new output directory are ordered IPT occurrences. Repeat a path
to repeat a placement. The command prints a JSON result, exits with zero on success,
and publishes only after its isolated worker exits normally. Timeout, unsupported
input or a worker failure leaves the destination unpublished.

To bundle a STEP conversion containing at most 32 parts:

```python
from pathlib import Path
import inventor_kit as ik

ik.write_step_parts("frame.step", "frame_parts")
assembly = ik.new_assembly(name="Frame")
for index, path in enumerate(sorted(Path("frame_parts").glob("*.ipt")), 1):
    assembly.add_part(path, name=f"Part:{index}")
assembly.save_bundle("frame_bundle", filename="frame.iam")
```

## Rust

Enable the `inventor-core` feature `iam-writer`:

```rust,ignore
use inventor_core::write::assembly::{AssemblyDocument, PartDefinition};

let part = PartDefinition::from_authored_ipt("rail.ipt", &ipt_bytes)?;
let mut assembly = AssemblyDocument::empty("Frame")?;
assembly.add_part(part, "Rail:1")?;
let iam_bytes = assembly.encode()?;
```

The Rust API operates in memory. The caller must package the exact supplied IPT
bytes beside the IAM. `prepare()` exposes the allocated identities and placements;
`PreparedAssembly::encode()` validates them before encoding. The older `from_ipt`
identity-inspection API remains available for planning, but its definitions cannot
be encoded without the stricter `from_authored_ipt` check.

## Scope and evidence

| Item | Current boundary |
|---|---|
| Profile | `inventor-iam-rse31-fixed-parts-v1`, metric RSe31, schema15 references |
| Input | Initial single-solid IPT writer profile; at most 64 MiB per IPT |
| Bundle | At most 32 occurrences, 256 MiB including IAM and report in Python |
| Placement | Flat, grounded, visible, identity transform |
| References | Portable same-folder IPT basenames; definitions shared by filename |
| Unsupported | Constraints, joints, subassemblies, arbitrary transforms, visibility edits, native-resaved input IPTs, in-place updates |
| Native acceptance | Four generated cases plus one control, 35 phases on Inventor build `311270010` |

Native cases covered one part, three placements sharing two definitions, 32
repeated placements, and a separate 32-part assembly. Each passed open/save/reopen,
then generated and saved IAM resolution after moving the bundle, resave and reopen.
Saved identities, references, topology and bounds were independently checked; the
32-part model also passed initial, fit and moved-reopened visual review. Inventor
may write absolute reference paths when saving; the moved-bundle test removed the
original location before opening. Other Inventor builds are unverified.

`document.native_qualified` in `bundle.json` describes this bounded profile;
`native_validation="not_run"` still describes each newly generated output. Local
wheel and sdist checks passed on Linux; remote multi-OS CI has not been run for
this change.

The implementation uses typed fields and allocation rules. Private CAD samples,
source identities, acquisition scripts, native oracle captures and investigation
reports are excluded from source and binary distributions. Public tests generate
synthetic parts. `ik.capabilities()["assembly_writer"]` reports the build's scope.

The [license terms](license.md) apply to the IAM writer as they do to the rest of
inventor-kit. Installed dependency notices include the UUID v5 implementation.
