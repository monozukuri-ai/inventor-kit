# Experimental IDW reading and display

[日本語](drawing.ja.md) · [Structural inventory](drawing-inventory.md)

The opt-in IDW path reads stored 2D elements without Inventor, model reprojection,
or Python CAD imports. It is **experimental partial support**, currently observed
on SampleBg's major31 / schema31 / Meta8 / zstd profile. Drawing correctness,
physical units, complete sheet membership and current state are not qualified.

```python
from inventor_kit import read_drawing_file

drawing = read_drawing_file("drawing.idw")
print(drawing.status, drawing.units, drawing.diagnostics)
for sheet in drawing.sheets:
    print(sheet.id, sheet.index, sheet.name, sheet.size_in_source_units)
    print(sheet.status, len(sheet.items), sheet.omissions)
    for item in sheet.items:
        print(item.id, item.geometry["kind"], item.source)
```

`read_drawing(bytes, source_id=..., limits=...)` accepts an immutable byte snapshot.
`read_drawing_file(path, limits=...)` bounds the file read. Both return a frozen
`DrawingDocument`; sheets, items, image metadata and nested geometry/style mappings
are immutable. Geometry is tagged `polyline`, `curve`, `text` or `image`. Image
assets retain their original PNG/JPEG bytes and SHA-256. Source spans and omission
reasons are retained. `drawing.sheet(id)` selects by source-local ID; duplicate
sheet names are allowed. IDs are stable for the same input, not guaranteed across
native resaves. `api_version` is 1; the support status is experimental.

Stored sheet order follows the document's tagged reference list. Each definition
must have a unique same-context SM backlink resolved through the full segment
GUID and object key. Registry order, display names and thumbnail content never
select owners. Unlinked display spaces are diagnosed and excluded from the API's
sheets. Ambiguous or missing sheet displays remain `unavailable`.

`size_in_source_units` retains raw width/height. `units=source_units_unverified`,
`length_unit=None` and `millimeters_per_unit=None` are intentional. Autodesk's
[API database length unit is centimeters](https://help.autodesk.com/cloudhelp/2021/ENU/Inventor-API/files/UOM_Overview.htm);
that fact alone does not prove the unit of this saved wire layout. No automatic
cm-to-mm conversion or dimensional measurement is exposed. `qualified=False`,
`complete=False` and `current_state=unverified` remain true even when a sheet
looks correct. Structural inventory's qualified `sheet_count` remains null.

```sh
python -m inventor_kit.viewer drawing.idw --experimental-drawing
```

The normal local Viewer provides sheet selection, pan, zoom, fit, text/curve
toggles, text search and element provenance. The IDW path does not require the
`viewer` Python extra or load the browser's 3D renderer. Without the flag it shows
metadata, saved previews and the opt-in instruction. Identification uses the CFB
root CLSID, not the filename extension. IPT/IAM selection flags cannot be combined
with experimental drawing mode. The existing process timeout and 16 MiB scene
limit apply; images also obey native byte/pixel limits and the Viewer buffer limit.
Only final successful worker output publishes the drawing and its image resources.

Unsupported majors preserve metadata/diagnostics and saved thumbnails. Invalid
containers, non-IDW inputs and invalid limits raise `ValueError`. Each native stage
independently applies `Limits`; these are not process memory or elapsed-time bounds.

Local regression checks cover SampleBg, corrupt/ambiguous references, reordered
synthetic sheet lists, duplicate names, unsupported profiles, resource limits,
worker failure and browser interaction. Synthetic cases do not establish native
multi-sheet correctness. Additional public IDWs examined so far use other majors;
a second major31 drawing, controlled native edits and Windows/Autodesk comparison
remain required. Major23/24/26/28/29 are not admitted by this drawing profile.
Unknown clipping, draw order, font metrics/alignment, some styles and annotation
coverage remain unresolved. The Viewer samples analytic curves for display and
uses diagnostic font/color fallbacks; the API retains original curve parameters.
