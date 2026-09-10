# API guide

English | [日本語](api.ja.md)

## Document metadata

```python
import inventor_kit as ik

doc = ik.inspect_file("part.ipt")
info = doc.metadata
print(info.identification.kind, info.stages)
for prop in info.find_properties(semantic_name="part_number"):
    print(prop.value, prop.source, prop.state_binding)
for diagnostic in info.diagnostics:
    print(diagnostic.code, diagnostic.severity, diagnostic.source)
```

`inspect(bytes, source_id="...")` is also available. Results from `read` /
`read_file` include the same `metadata`. By default, `inspect` does not parse
geometry or import cq-acis / CadQuery. Supported thumbnails are returned in
`info.thumbnails` with PNG bytes, dimensions, and provenance.

Properties retain their FMTID/PID, type, code page, original bytes, value, and
provenance. Values from the main document and `MemberDocs` remain separate, and
all candidates from different storage locations are returned. The reader does
not determine effective overrides for individual Model States or values shared
across all states.

## Saved candidates and geometry

```python
inventory = ik.inspect_file("part.ipt", include_candidates=True)
for candidate in inventory.geometry.candidates:
    print(candidate.id, candidate.table_status, candidate.state_binding)

# Select a candidate explicitly after inspecting its ID in the list.
# doc = ik.read_file("part.ipt", candidate_id=chosen_id)
doc = ik.read_file("part.ipt", require_current_state=True)
```

Candidate IDs are bound to the SHA-256 of the entire input. An ID from another
file or an outdated ID does not fall back to a different candidate. Explicit
selection does not prove that a candidate represents the current state.
`require_current_state=True` withholds unverified models and records diagnostics.

Malformed CFB data and input limit violations raise `ValueError`. Unsupported or
ambiguous structures return `geometry_unavailable` with diagnostics.
`decoded_subset` means that a saved B-rep was imported into the shared model; it
does not indicate successful geometry conversion or interpretation of the entire
file. Even when `doc.model` is available, `doc.to_cadquery()` stops on unsupported
curves, surfaces, or trims.

`doc.kernel_bytes` contains the original selected kernel payload. Entity
`SourceSpan` offsets refer to the decompressed B stream, not the IPT file.
Subtract `doc.summary["carrier"]["kernel_offset"]` to obtain offsets within
`kernel_bytes`. Geometry coordinates are converted from Inventor's centimetres
to millimetres; the SAB header scale is retained separately as `header_scale`.

## Assemblies

```python
saved = ik.inspect_assembly_file("assembly.iam")
assembly = ik.read_assembly_file("assembly.iam", search_roots=["parts"])
for instance in assembly.instances:
    print(instance.path, instance.resolution, instance.world_transform_mm)

converted = assembly.to_cadquery(allow_unverified_state=True, allow_partial=True)
print(converted.omissions, converted.reference_issues)
report = converted.export_step("assembly.step", allow_partial=True)
print(report["roundtrip"]["status"])
```

`inspect_assembly` and its file variant do not read other files or import Python
geometry modules. Part definitions and occurrences remain distinct. Placement
matrices use millimetres, are stored in row-major order, and act on column vectors.
File searches are restricted to the input IAM's parent directory and explicit
search roots, without network retrieval. Document ID mismatches, name collisions,
missing references, cycles, and unresolved placements remain recorded. Unknown
placements are not replaced with identity matrices.

`allow_partial=False` is the default. Incomplete conversion raises
`AssemblyConversionError` with partial results. `allow_unverified_state=True`
permits using saved placements; it does not verify the current state.
`stored_occurrences` also retains uninterpreted UFRx properties with their types,
tags, and original offsets.

Assembly STEP export refuses to overwrite existing files. Its accompanying JSON
report records input hashes and omissions. XDE reimport checks hierarchy, names,
placements, geometry metrics, and opaque RGB values per part. Colors are the
values set in CadQuery; native Inventor colors are not decoded.

## Per-document limits

`Limits` can be passed to `read` / `inspect`, their file variants, and the
assembly inspection and resolution APIs. Both Python and Rust validate that
values are nonnegative integers no greater than the defaults.

| Field | Default maximum |
| --- | ---: |
| `max_file_bytes` | 128 MiB |
| `max_stream_bytes` | 64 MiB |
| `max_inflated_bytes` | 64 MiB |
| `max_total_inflated_bytes` | 128 MiB / document |
| `max_records` | 500,000 |
| `max_streams` | 65,536 |
| `max_property_bytes` | 16 MiB |
| `max_property_items` | 100,000 |
| `max_property_depth` | 16 |
| `max_candidates` | 256 |

File variants check size before and after reading. Caller-created `bytes` already
occupy memory before being passed to the parser. Resource exhaustion during
parsing follows the existing partial-result and diagnostic contracts.

The same `Limits` are propagated to subsequent assembly part conversion.
Graph-wide traversal is limited separately by `FileSystemResolver` parameters:
`max_documents`, `max_instances`, `max_depth`, `max_total_file_bytes`, and
`max_directory_entries`. These limits do not guarantee bounds on process RSS,
runtime, or OCCT computation.
