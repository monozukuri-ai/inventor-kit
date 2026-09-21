# Experimental IDW reading and display

English | [日本語](drawing.ja.md)

The opt-in IDW path reads stored 2D elements without Inventor, model reprojection,
or Python CAD imports. It is **experimental partial support**, currently observed
on the major31 / schema31 / Meta8 / zstd profile. Drawing correctness, physical
units, complete sheet membership and current state are not qualified.
The reader uses saved display data and does not load related IPT/IAM files or
regenerate drawing views.

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

`read_drawing(bytes, source_id=..., limits=..., drawing_limits=...)` accepts an immutable byte snapshot.
`read_drawing_file(path, limits=..., drawing_limits=...)` bounds the file read. Both return a frozen
`DrawingDocument`; sheets, views, items, image metadata and nested geometry/style mappings
are immutable. Geometry is tagged `polyline`, `curve`, `text` or `image`. Image
assets retain original PNG/JPEG bytes. Observed monochrome view caches are converted
from stored pixels to PNG, retaining the generated asset hash and original record source. Source spans and omission
reasons are retained. `drawing.sheet(id)` selects by input-bound ID; duplicate
sheet names are allowed. IDs include the input SHA-256 and stored record/placement identity. Foreign-input IDs are rejected. They are stable across path changes, not guaranteed across
native resaves. `api_version` is 1; the support status is experimental.

Sheets follow the stored document order. Only unambiguously associated display
content is included. Missing or ambiguous sheet displays remain `unavailable`,
with diagnostics explaining the omission.

`size_in_source_units` retains raw width/height. `units=source_units_unverified`,
`length_unit=None` and `millimeters_per_unit=None` are intentional. Autodesk's
[API database length unit is centimeters](https://help.autodesk.com/cloudhelp/2021/ENU/Inventor-API/files/UOM_Overview.htm);
that fact alone does not prove the unit of this saved wire layout. No automatic
cm-to-mm conversion or dimensional measurement is exposed. `qualified=False`,
`complete=False` and `current_state=unverified` remain true even when a sheet
looks correct.

```sh
python -m inventor_kit.viewer drawing.idw --experimental-drawing
```

The normal local Viewer provides sheet selection, pan, zoom, fit, text/curve
toggles, text search and element provenance. The IDW path does not require the
`viewer` Python extra or load the browser's 3D renderer. Without the flag it shows
metadata, saved previews, sheet descriptors and strict display diagnostics. Identification uses the CFB
root CLSID, not the filename extension, including dependency checks at CLI startup.
Incompatible document options appear as worker diagnostics. IPT/IAM selection flags cannot be combined
with experimental drawing mode. The existing process timeout and 16 MiB scene
limit apply; images also obey native byte/pixel limits and the Viewer buffer limit.
Only final successful worker output publishes the drawing and its image resources.

Unsupported majors preserve metadata/diagnostics and saved thumbnails. Invalid
containers, non-IDW inputs and invalid limits raise `ValueError`. Each native stage
independently applies `Limits`; these are not process memory or elapsed-time bounds.

`DrawingLimits` adds native drawing budgets. Its defaults are also hard ceilings;
callers may lower them to nonnegative integers (booleans are rejected).

```python
from inventor_kit import DrawingLimits, read_drawing_file

drawing = read_drawing_file("drawing.idw", drawing_limits=DrawingLimits(
    max_sheets=32, max_views=128, max_display_items=20_000, max_output_bytes=16 * 1024 * 1024))
```

| Field | Default / hard ceiling | Scope |
| --- | ---: | --- |
| `max_sheets` | 256 | Stored sheet references / candidate display spaces |
| `max_views` | 4,096 | Reachable decoded saved view placements across the drawing |
| `max_display_items` | 100,000 | Expanded items across the drawing |
| `max_polyline_points` | 1,000,000 | Expanded polyline points across the drawing |
| `max_text_bytes` | 16 MiB | Expanded UTF-8 text, view names and copied font-family bytes |
| `max_reference_visits` | 500,000 | Work budget per native drawing stage, also bounded by `Limits.max_records` |
| `max_nesting_depth` | 128 | Display traversal / ancestor depth |
| `max_image_bytes` | 16 MiB | Aggregate embedded image bytes; view caches charge raw record bytes and generated PNG |
| `max_image_pixels` | 16,777,216 | Aggregate embedded image pixels |
| `max_output_bytes` | 64 MiB | Encoded native drawing JSON, including metadata and image data |

Expansion is charged before copying items, points or text, including repeated
placements. Exhausting an expansion budget discards the display rather than
returning a successful prefix; metadata and diagnostics remain available.
Image limits produce unavailable image descriptors. The JSON writer checks its
remaining budget before appending; exceeding it raises `ValueError`. Container
parsing still uses `Limits`. Unknown view layouts remain omissions; the view budget counts decoded placements,
not all possible model projections. These budgets do not bound process RSS or elapsed time. The Viewer uses the defaults; programmatic `Options`
also accepts `drawing_limits`.

`sheet.views` exposes immutable `DrawingView` objects with input-bound IDs,
view names, cache bounds, placement matrices, image references, associated item
IDs and source spans. An absent cache has `image_reference=None` and
`cache_bounds=None`. The Viewer lists these views and can highlight their saved
elements. A placement matrix positions the cache; rotation may already be baked
into its pixels. `rotation`, `parent_view_id` and `view_type` remain `None` rather
than inferring a zero angle, base view or parent relationship. Cache bounds and
matrices retain source coordinates and do not establish a clipping region.

## Displayed content and limitations

Supported saved elements include lines, polylines, circles/arcs, text and images.
Dimensions, leaders, frames, title blocks and simple parts lists can be displayed
when represented by supported saved elements; the API does not expose their
full dimension or table semantics.

Supported monochrome view caches display base, projected, scaled, rotated,
hidden-line and cropped views at their saved raster resolution. Zooming cannot
recover vector detail. Unsupported color/alpha layouts produce unavailable assets;
uninterpreted appearance data can omit a branch with a diagnostic reason.

For supported Arial/Tahoma text, the Viewer adjusts saved height using browser
font metrics, preserves spaces and separate baselines, and applies supported
bold/italic flags. Missing fonts use a sans-serif fallback. Unknown layouts and
browsers without the height adjustment use an explicitly unverified fallback.
Installed fonts affect text width and glyph shape; exact text fidelity is not
guaranteed. The observed AIGDT `n` glyph is displayed as the approximate Unicode
`⌀`, while original text and font remain available through the API. Other legacy
symbols are not mapped.

General clipping, draw order, text alignment and annotation coverage remain
unverified. Curves are sampled for display; the API retains their original
parameters. Major23/24/26/28/29 are outside this drawing profile.

## Display requests and sheet resources

`drawing.render_sheet(sheet_id=sheet.id, allow_partial=False)` requests a millimeter
`SheetDisplay`. An invalid or foreign sheet ID raises `DrawingDisplayError` with
code `drawing.invalid_sheet_id`. Unknown units or placement raise `DrawingDisplayError` even when
`allow_partial=True`; inspect its `sheet_id`, `diagnostics` and `omissions`.
Current major31 profiles still have unverified physical units, so this entrypoint
refuses them. `sheet.items` and `--experimental-drawing` retain the explicit
source-coordinate investigation path. Content coverage stays `unknown`, with
`snapshot_kind=saved` and `reference_freshness=unverified`.

Normal Viewer startup and `--allow-partial` show the stored sheet inventory and
reasons display is refused. Partial permission never resolves unknown units.
The experimental Viewer loads the selected sheet on demand and discards stale
responses when switching sheets. Sheet and image resources are checked before
being made available.
Each sheet JSON is limited to 16 MiB. Sheets and images together are limited to
128 MiB or the lower Viewer buffer limit. The 16 MiB metadata ceiling is separate;
none of these byte limits promises bounded RSS or elapsed time.
