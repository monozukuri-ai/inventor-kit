# Controlled IDW specimens

[日本語](drawing-specimens.ja.md) · [Capture contract](drawing-validation.md)

These are authoring instructions, **not acquired native fixtures**. No Windows
host was available when the corpus infrastructure was added. Synthetic JSON and
public drawings do not replace this experiment.

Use one Inventor version/build, one template, locale and font installation for
the first profile. Record template hash, provider/build, locale, database/display
units, export settings, installed font files/versions and their hashes. All cases,
including later year-version saves, belong to one regression family. Do not use
the reserved rim/vise holdouts to construct or tune these cases.

Create a blank A4 landscape drawing (29.7 × 21 cm), named `Sheet A`, with border
and title block removed and no referenced models. Save/close/reopen it as
`blank.idw`. Each row starts from the named base, changes only the stated factor,
and saves to a new file. Record actual provider observations; the specified
coordinates below are input instructions, not measured expected values.

| New file / base | Single controlled change | Independent evidence |
| --- | --- | --- |
| `resave.idw` / blank | Save without editing | Hashes and property/stream noise; do not expect byte equality |
| `sheet-name.idw` / blank | Rename to `図面 A` | Exact name and index |
| `sheet-portrait.idw` / blank | A4 portrait | Width/height/orientation |
| `sheet-a3.idw` / blank | A3 landscape | 42.0 × 29.7 cm observations |
| `two-sheets.idw` / blank | Append A4 sheet `Sheet B` | Names, dimensions and order |
| `reordered.idw` / two-sheets | Move Sheet B before Sheet A | Order only |
| `line.idw` / blank | Sheet sketch line (2,3) → (8,3) cm | Local geometry and sheet basis |
| `line-length.idw` / line | Move second endpoint to (10,3) | Endpoint delta |
| `line-position.idw` / line | Translate by (1,2) cm | Placement delta |
| `circle.idw` / blank | Circle center (6,6), radius 2 cm | Center and radius |
| `arc.idw` / blank | Center (6,6), radius 2, start 0, CCW sweep π/2 | Endpoints and directed angles |
| `text-ascii.idw` / blank | General note `ABC 123`, (2,10), height 0.35 cm | Text, position, style and font |
| `text-japanese.idw` / text-ascii | Replace string with `図面 日本語 ⌀10` | Exact Unicode and glyph appearance |
| `text-multiline.idw` / text-ascii | Replace string with `ABC\n123` (actual newline) | Text, line breaks and box |
| `text-rotated.idw` / text-ascii | Rotation π/2 | Rotation and raster placement |
| `base-view.idw` / blank | Base front view of a separately saved 20×10×5 mm block at (10,10) cm, scale 1 | Reference hash, position and projected edges |
| `projected-view.idw` / base-view | Add one projected view above | View membership/placement and edges |
| `view-scale.idw` / base-view | Scale to 0.5 | Scale and sheet geometry |
| `view-rotation.idw` / base-view | Rotation π/2 | Rotation and geometry |
| `hidden-lines.idw` / base-view | Enable hidden-line display | Segment visibility/hidden flags; use a documented hole model if needed, in a separate base |
| `linear-dimension.idw` / base-view | One 20 mm edge dimension | Displayed text, origin and rendered leaders |
| `diameter-dimension.idw` / circle | One diameter dimension | Displayed text and symbol |
| `leader.idw` / base-view | One leader note | JSON query coverage gap plus independent PDF/PNG |
| `title-block.idw` / blank | One title block with prompted/property text | Evaluated strings and visual placement |
| `parts-list.idw` / assembly base | Two instances of the block in a separate IAM, one base view; add parts list | IAM/IPT hashes, cell values and visual layout |
| `missing-reference.idw` / base-view | Remove IPT from a *copied* project; do not overwrite originals | Missing reference and retained/degraded saved display |
| `deferred.idw` / base-view | Change referenced block length, keep drawing updates deferred | Both IDW/IPT hashes, update flags and saved/opened differences |
| `without-preview.idw` / blank | Change preview-save option only, if supported | Thumbnail availability independent of drawing data |

For the assembly case, save the IAM drawing without a parts list as the base
before adding the list. For missing/deferred/preview cases, a changed environment
is itself the experimental variable; retain every before/after input. If an API
or UI refuses a case (for example sketch-circle dimensions), record that outcome
and use a separately documented circular-part base instead of silently changing
the experiment.

For each case:

1. Save the IDW and references, close the session, and hash all files. Keep the
   authoring inputs and observations separate. Add canonical manifest rows only
   after obtaining real bytes; never substitute fake hashes.
2. Run the collector on an isolated copy. Preserve JSON, provider/build and
   collector hash. Compare Apprentice and Inventor on at least the blank, line,
   general-note and base-view specimens to qualify the chosen provider.
3. Independently export each sheet to PDF in the **same documented state**;
   rasterize to PNG at 150 DPI using one recorded renderer version. Record fonts,
   page size/orientation, antialias settings, source hash and sheet index.
   Screenshotting a thumbnail is not a full-sheet reference image.
4. Verify that opening/exporting did not change the relevant saved state.
   Preserve logs explaining update/migration/deferred-reference behavior.
   Unknown state remains unknown; updated results must use a separate identity.
5. Attach each PDF/PNG's hash/bytes and session provenance in `visual_artifacts`.
   Use per-sheet PNGs for the acquisition gate. Preserve the PDF as a companion;
   record a PDF's rasterization DPI when used as an image reference.
6. Validate schema, input/artifact hashes and query completeness offline. Keep
   unsupported entities as gaps. Do not mark the stage accepted until at least
   the blank, line and text cases have independently verified sheet coordinates,
   displayed strings and full-sheet visuals for one saved profile.

The current collector deliberately has gaps for leader graphics, title-block
geometry and table layout. Those cases require independent exports now and
expanded observation coverage before claiming full semantic capture. Image
alignment and numerical tolerances must be set from controlled regression
specimens, then frozen before opening the holdouts for acceptance.
