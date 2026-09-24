# IDW reading and saved display

English | [日本語](drawing.ja.md)

The Viewer opens supported IDW drawings by default, reading stored 2D elements
without Inventor, model reprojection or Python CAD imports. The reader provides
**experimental partial support**, currently observed
on schema31 / Meta8 with segment majors 23, 24, 26, 28, 29 (zlib), or 31 (zstd). Drawing correctness, physical
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
assets retain original PNG/JPEG bytes. Supported major23/26/28 splines use the stored
degree, knots, control points, weights and parameter range, sampled into a `polyline`
with 16 segments per nonempty knot span. This approximation has no general geometric
error bound. Elliptical arcs retain their center, two axes and angle range as `curve`.
Supported major31 monochrome and major23/26/28 color RGBA view caches are converted
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
python -m inventor_kit.viewer drawing.idw
```

The normal local Viewer provides sheet selection, pan, zoom, fit, text/curve
toggles, text search and element provenance. The IDW path does not require the
`viewer` Python extra or load the browser's 3D renderer. Use `--metadata-only` to
show document information and saved previews without decoding the drawing.
The old `--experimental-drawing` flag is accepted for compatibility and is no
longer necessary. `--allow-partial` does not change IDW display or verify its units.
The Viewer displays supported content in saved source coordinates and labels it
as partial, with unverified units and current state. Identical sheet names include
their order number in the sidebar; the first available sheet opens automatically.
Identification uses the CFB root CLSID, not the filename extension.
Incompatible IPT/IAM selection options appear as worker diagnostics.
The existing process timeout and 16 MiB scene
limit apply; images also obey native byte/pixel limits and the Viewer buffer limit.
Only final successful worker output publishes the drawing and its image resources.
Each selected sheet resource is limited to 32 MiB; all sheet and image resources
are limited to 128 MiB in total.

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
| `max_field_values` | 1,000,000 | Typed payload work across the document; numeric values, references and UTF-16 code units are individually charged |
| `max_sheets` | 256 | Stored sheet references / candidate display spaces |
| `max_views` | 4,096 | Reachable decoded saved view placements across the drawing |
| `max_display_items` | 100,000 | Expanded items across the drawing |
| `max_polyline_points` | 1,000,000 | Expanded polyline points across the drawing |
| `max_text_bytes` | 16 MiB | Expanded UTF-8 text, view names and copied font-family bytes |
| `max_reference_visits` | 3,000,000 | Aggregate reference and copy work across the document, independently for sheet binding, display expansion and image retrieval |
| `max_nesting_depth` | 128 | Display traversal / ancestor depth |
| `max_image_bytes` | 16 MiB | Aggregate delivered image bytes; view caches charge generated PNG bytes |
| `max_image_pixels` | 16,777,216 | Aggregate embedded image pixels |
| `max_output_bytes` | 96 MiB | Encoded native drawing JSON, including metadata and image data |

Expansion is charged before copying items, points or text, including repeated
placements. Exhausting an expansion budget discards the display rather than
returning a successful prefix; metadata and diagnostics remain available.
Image limits produce unavailable image descriptors. The JSON writer checks its
remaining budget before appending; exceeding it raises `ValueError`. Container
parsing uses `Limits.max_records`; typed-field parsing independently uses
`DrawingLimits.max_field_values`. The latter replaces the previous 500,000
allowance with a 1,000,000 ceiling: the filter-panel assembly needs 573,482 work
units. The container's 500,000 ceiling is unchanged. Each field value still
consumes its allowance, and callers can lower either limit.
Raw cache pixels are bounded by expanded-stream bytes and `max_image_pixels`; each source stream is expanded once. Unknown view layouts remain omissions; the view budget counts decoded placements,
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

Supported monochrome and color RGBA view caches display base, projected, scaled, rotated,
hidden-line and cropped views at their saved raster resolution. Zooming cannot
recover vector detail. Unsupported pixel/alpha layouts produce unavailable assets;
uninterpreted appearance data can omit a branch with a diagnostic reason.

For supported Arial/Tahoma and observed major24 ISOCP/ISOCP_IV25/Vafle text, the Viewer adjusts saved height using browser
font metrics, preserves spaces and separate baselines, and applies supported
bold/italic flags. Japanese text adds local Noto Sans CJK JP, Yu Gothic, Meiryo and
Hiragino fallbacks before sans-serif. Unknown layouts and
browsers without the height adjustment use an explicitly unverified fallback.
Installed fonts affect text width and glyph shape; exact text fidelity is not
guaranteed. The observed AIGDT `n` glyph is displayed as the approximate Unicode
`⌀`, and observed AIGDT `x` as the depth symbol `↧`, while original text and font
remain available through the API. Other legacy symbols are not mapped. These
Unicode fallbacks select local symbol fonts; no proprietary fonts are bundled.

General clipping, draw order, text alignment and annotation coverage remain
unverified. Circular and elliptical arcs use SVG ellipse arcs, including affine
placements and decoded dash lengths, without fixed polyline subdivision. Stored
splines still use the approximation described above. Nearly degenerate projected
ellipses use line segments through endpoints and coordinate extrema, recorded in
the SVG and sidecar. Their analytic deviation is at most 2e-9 source units,
excluding serialization and browser rasterization error. Unknown line-pattern masks
remain unresolved; they are not interpreted as newly supported native line types.
For major23/28, supported saved vector edges and annotations overlay the color cache.
Historical target contexts require matching object identity; this does not reconstruct historical state.
Unknown sketch states, cross-segment display children and missing images remain explicit omissions.

## Observed segment profiles

The additional profiles were enabled in the order 24 → 29 → 28 → 26. Each
profile requires its own registry version, envelope, codec and observed field
layouts. A nearby version number or the same compression codec does not grant
support. Segment major numbers are not Inventor release-year identifiers.

| Segment major | New regression inputs | Saved sheets / views | Display items | View-cache decoder |
| --- | --- | --- | --- | --- |
| 24 | `Template_IACS.idw` | 1 / 0 | 70 | Not enabled; model views remain unobserved |
| 29 | `Toys-R-Us-Rex.idw` | 1 / 3 | 166 | Not enabled |
| 28 | `mateolikescats.idw` | 1 / 5 | 1,650 | RGBA; one saved cache observed |
| 26 | `RespiraWorks.idw`, `starliliko.idw` | 1 / 3 and 1 / 0 | 543 and 90 | RGBA |
| 26 | `RespiraWorks-bottom-assembly.idw`, `RespiraWorks-filter-panel-assembly.idw` | 1 / 3 and 1 / 4 | 550 and 28,820 | RGBA; two saved caches observed |

Point lists resolve their encoding from the owning segment type table. Observed
major26/28/29 triangle batches render filled saved arrowhead geometry. Major28
also admits the observed spline, ellipse and annotation-placement layouts.
Major26 also admits observed splines, balloons, revision tables, and section/detail
annotation placements. The filter-panel assembly displays 28,820 saved elements.
Shared styles, view references, placements and source bases reduce the same sheet
content from 42,384,840 to 29,480,621 bytes (about 28.1 MiB), preserving item IDs,
source evidence, omissions and the 32 MiB resource ceiling.

These are pinned real-file regression results. Source-preserved Inventor 2027.1
API/PDF captures of the five original inputs corroborated paper dimensions and
11 saved view positions. The 93 matched filled PDF triangles have maximum vertex
distance 0.04381 mm, with no fitted transform. Two additional saved triangles
have no standalone triangular PDF match. Native opening marked all documents
dirty; three had missing external models. This comparison does not qualify
current state, general units or complete rendering. The new captures also
support the observed major24/26/28 border layout, major24 color mask 8, and
major26/28 local annotation/major26 hole-table placements.
`measure_drawing_profiles.py` validates the private capture identities and
reproduces these limited comparisons; the capture bytes are not distributed.
All remain `experimental_partial`. Unknown curve variants, colors, fonts and
unresolved references remain diagnostic omissions or explicit font fallbacks.
The observed major26 SM circle/arc suffix 1 displays filled disks and circular
segments closed by a chord. All 14 in RespiraWorks matched native PDF vertices
within 0.03489 mm. Major28 also admits the observed balloon placement and filled
SM circles: all ten disks in `assembly3exp.idw` matched PDF vertices within
0.04391 mm. Filled major28 arcs and other suffixes remain unsupported.
The observed hidden-line layer pattern is enabled for major29/28 (by weight) and
major26 (fixed base). Nominal source dash lengths are preserved; SVG fits
observed straight-line profiles as described below. Curve phase and overall
visibility remain unverified.

The additional capture covers ten drawings, 25 views and 32,276 curve segments.
The observed major28 DL boolean mask=2/value=0 suppresses 166 items, all matched
to API segments with Visible=false; 14 comparisons cover endpoints only. Other
values and profiles retain unresolved semantics. The 14,958 added major26 ellipses
in the filter-panel assembly have 17 fixed sample points each, at most 0.04461 mm
from native PDF strokes. API endpoint pairs match within 0.0001 mm for 14,940
curves; 18 remain unmatched at that threshold. These measurements do not bound
continuous-curve error. The added 4,270 major26 splines all match API endpoint
pairs within 0.0001 mm. Including ellipses, 339,804 sample points across 19,228
curves are within 0.05278 mm of sampled PDF strokes. Of 117 added balloon/revision
table items, 84 line/circle items have sample distances at most 0.06429 mm.
The additional section and detail drawings display 951 and 755 items; the detail
view's two arrowheads match PDF vertices within 0.03364 mm. Some section/detail
curve comparisons exceed the PDF sample threshold; dash fitting, visibility and
clipping remain unqualified. All ten
captures were dirty and eight had missing external models; source hashes and
before/after document state were preserved. Major24
has template evidence only. Embedded PNG/JPEG assets are independent of the
view-cache decoder. Native unit/state accuracy and independent holdout acceptance
have not been qualified for these profiles. Majors 21, 25, 27, 30 and other
unlisted versions remain unsupported.

The fixture manifest pins source commits, sizes and SHA-256 hashes. CAD inputs
are downloaded separately for validation; they are not bundled in releases and
the manifest does not grant redistribution rights. Regression inventories cover
283,487 records across nine drawings, including the existing major23/31 inputs.

## Display requests and sheet resources

`drawing.render_sheet(sheet_id=sheet.id, allow_partial=False)` requests a millimeter
`SheetDisplay`. An invalid or foreign sheet ID raises `DrawingDisplayError` with
code `drawing.invalid_sheet_id`. Unknown units or placement raise `DrawingDisplayError` even when
`allow_partial=True`; inspect its `sheet_id`, `diagnostics` and `omissions`.
All current drawing profiles still have unverified physical units, so this entrypoint
refuses them. `sheet.items` and the default Viewer expose the supported saved
content in source coordinates without physical-unit conversion. Content coverage stays `unknown`, with
`snapshot_kind=saved` and `reference_freshness=unverified`.

The Viewer loads the selected sheet on demand and discards stale
responses when switching sheets. Sheet and image resources are checked before
being made available.
Sheet wire format v4 shares styles, view member references, placements and source
bases without dropping provenance. Item IDs are reconstructed exactly from the
input/sheet/segment/placement/record identity. The loader also reads v1/v2/v3.
Invalid indices, byte ranges and duplicate IDs are rejected before display. The private Rust-to-Python
transport also shares styles while retaining the 96 MiB output ceiling.
The Python drawing API and downloaded SVG/report retain their existing format.
Each sheet JSON is limited to 32 MiB. Sheets and images together are limited to
128 MiB or the lower Viewer buffer limit. The 16 MiB metadata ceiling is separate;
none of these byte limits promises bounded RSS or elapsed time.

## Reports and offline SVG

```python
from inventor_kit import read_drawing_file

drawing = read_drawing_file("drawing.idw")
report = drawing.report()  # JSON-compatible inventory, raw text and omissions
sheet = drawing.sheets[0]
detailed = drawing.report(sheet_id=sheet.id, details=True)
svg = drawing.to_svg(sheet_id=sheet.id, allow_partial=True)  # str
saved = drawing.export_svg("sheet.svg", sheet_id=sheet.id, allow_partial=True)
```

`export_svg` creates a new SVG and `.svg.json` sidecar without overwriting either.
The JSON follows [drawing report v1](../schemas/drawing-report-v1.schema.json),
including input and output hashes, original text/font data, saved view references,
sources and omissions. Embedded image bytes are in the SVG, not the JSON.
`to_svg` and `export_svg` require explicit partial opt-in, accept `max_bytes` up to
32 MiB and require a sheet ID for documents with more than one sheet. Unavailable
sheets and foreign-input IDs raise `DrawingDisplayError`. API `report()` returns
a dictionary; CLI serialization is bounded and supports batch JSONL.

The Viewer, Python API and CLI share this SVG renderer. **Save partial SVG**
downloads the selected sheet with images embedded; **Save report** downloads its
provenance JSON. Export includes the full saved sheet even after zooming,
searching, selection or hiding text/curves. It can be opened after the Viewer
server stops, without network access. The SVG identifies the source and sheet,
and uses a pixel viewport over unverified source coordinates, with no mm claim.
Fonts are not embedded; installed fonts and browser font-metric support still
affect the result. XML-incompatible characters are replaced only in the SVG;
the JSON preserves the original string. Saving SVG does not enable the separate
millimeter `render_sheet` API or qualify current state or annotation semantics.

## Unit precision and general line-style evidence

[measure_drawing_precision.py](../scripts/measure_drawing_precision.py) checks the
four pinned unit controls against API cm values, SVG line coordinates and native
PDF vectors separately, without fitting scale or position. It records errors at
a 0.001 mm threshold. The existing 400 dpi PDFs differ by up to 0.028 mm in line
coordinates and about 0.139 mm in paper size, so they cannot serve as that precision
reference. Saved/API agreement applies only to these snapshots, not general IDW
units or printer calibration. The private controls, `pdfinfo` and `mutool` are required.

```sh
python scripts/measure_drawing_precision.py --input /path/to/units --output precision.json
```

[create_drawing_linetype_controls.ps1](../scripts/create_drawing_linetype_controls.ps1)
creates 38 new IDW/API/PDF specimens covering 15 built-in patterns through layer
inheritance and entity overrides, plus isolated weight, scale and weight-dependent
scaling controls. Use Windows PowerShell 5.1 with Inventor running and all existing
documents closed. It does not update existing IDW files or global styles.
The 38 controls were captured and checked in Inventor 2027.1 (major31). The
experimental decoder now supports nominal arrays for all 15 built-in layer
patterns when the observed layer and binding scales are 1, including width-based
scaling. Explicit arrays already contain entity scale and are not scaled twice.
Saved pattern IDs are distinct from [API enum values](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/LineTypeEnum.htm);
names and enum values alone do not establish dash arrays.

```powershell
.\scripts\create_drawing_linetype_controls.ps1 -OutputDirectory C:\Evidence\linetypes-new
```

```sh
python scripts/validate_drawing_linetype_controls.py --input /path/to/linetypes-new --output acquisition.json
```

Use new output paths. The acquisition gate rejects failed/missing getters, changed
saved state, duplicate cases and mismatched source hashes. A pass still reports
`pattern_mapping_qualified=false`; acquisition alone never admits a decoder.
The separate pinned regression check is:

```sh
python scripts/measure_drawing_linetypes.py --input /path/to/linetypes-new --output linetypes.json
```

Native PDF endpoints adjust dash period and phase to fit each line. The Viewer
and saved SVG now fit observed two-point straight lines to whole periods, with
half a dash at each endpoint. Actual SVG stroke endpoints across the 38 major31
standard-pattern controls match PDF within 0.00265 mm (0.01 mm threshold).
The observed major26/28/29 hidden lines and short major24 projection-symbol lines
also use fitting; lines shorter than one nominal period use equal dash/gap/dash
thirds. Of 197 straight lines in six preserved drawings, 175 match PDF endpoints
within 0.05 mm; 22 tiny or overlapping/ambiguous lines remain unresolved.
Major24 is limited to short lines; major31 is limited to at least one period.

Source coordinates, nominal arrays and provenance are unchanged. Source styles
retain `dash_phase_and_fit_unverified`; SVG `data-dash-rendering` and sidecar
`export.dash_rendering` distinguish fitted lines from nominal rendering.
Curves, polylines with three or more points, unobserved patterns and half-period
rounding ties retain nominal rendering. Curve phase, major23 layer patterns,
custom `.lin`, non-unit layer scales and independent holdouts remain unqualified.
Setting a sketch default
explicitly can create a continuous override; the collector leaves inherited
properties untouched. The observed major31 default-width sentinel now preserves
the layer width instead of making the sheet unavailable.
