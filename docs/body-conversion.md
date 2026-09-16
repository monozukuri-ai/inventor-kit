# Body selection and conversion reports

English | [日本語](body-conversion.ja.md)

The v0.4.0 body API converts each saved IPT body independently through cq-acis.
It keeps every unsupported body and its source. `doc.to_cadquery()` retains its
existing strict whole-part behavior. A saved Model State remains unverified.

```python
import inventor_kit as ik

doc = ik.read_file("part.ipt")
result = doc.convert_bodies()
for body in result.bodies:
    print(body.id, body.body_index, body.status, body.diagnostics)
# Choose an ID from this inventory; no primary body is selected automatically.
# result.export_step("selected.step", body_ids=[chosen_id], allow_partial=True)
```

`BodyConversion` contains `id`, `body_index`, `source`, `status`, `diagnostics`,
`metrics`, and `shape`. Only `converted_solid` bodies have a shape and metrics.
Other statuses are `open_shell`, `unsupported`, and `failed`. A sewing failure
without a shell is `unsupported`; it is not proof of a valid sheet body.
Open surfaces and collections of individual faces are not exported as solids.

IDs bind the input SHA-256, selected candidate, and original body index.
Copies of identical input bytes have the same IDs. IDs from another input or
candidate, duplicate IDs, and empty explicit selections are rejected.
Source offsets refer to the inflated B stream, as with the shared ACIS model.

`result.report(body_ids=...)` returns a JSON-serializable report without OCCT
objects. Its [v1 schema](../schemas/conversion-report-v1.schema.json) also covers
`AssemblyConversion.report()` and the [CLI](cli.md) envelopes. Diagnostic codes
remain cq-acis codes; source/entity/subtype details are included when available.
Unknown details are null. Unexpected native exceptions propagate to the caller.

| Field | Meaning |
| --- | --- |
| `geometry_complete` | Every inventoried body converted to a valid solid |
| `selection_complete` | Every requested body converted to a valid solid |
| `current_state_verified` | Always false in this version |
| `complete` | Always false; saved geometry does not establish current state |
| `omissions` | Unselected or unconverted bodies, including their diagnostics |

`to_cadquery(body_ids=..., allow_partial=True)` and
`export_step(path, body_ids=..., allow_partial=True)` use only the explicit
selection. Without `body_ids`, all bodies are requested. Any omitted body requires
`allow_partial=True`; a selection with no valid solids is always rejected with
`BodyConversionError`, whose `result` holds the full conversion result.
Reports and body inventories can be inspected without permitting partial output.

STEP export creates a new `.step`/`.stp` and `.json` sidecar. It checks XDE names,
body leaves, face/solid counts, volume, area and bounding boxes after reimport.
The sidecar retains input hash, candidate, body IDs and every omission.
Existing files are refused; failed round-trip checks publish neither file.

FTC06 (2021/2024) is an example: body 1 is a valid 146-face solid, while two
auxiliary planar bodies prevent whole-part conversion. Explicit body selection
can export body 1 and record the other two. This does not count as a new
whole-part conversion success or infer which body Inventor currently displays.

```sh
python -m inventor_kit part.ipt --list-bodies
python -m inventor_kit part.ipt --body-id BODY_ID --allow-partial --step selected.step
python -m inventor_kit.viewer part.ipt --body-id BODY_ID --allow-partial
```

The viewer retains every body in the tree, including unsupported and unselected
bodies. Partial display requires `--allow-partial` and remains visibly partial.
