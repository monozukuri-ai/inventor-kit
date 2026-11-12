# Changelog

Release notes of published versions are in [docs](docs/README.md)
(`release-notes-vX.Y.Z.md`). This file collects changes that are not released yet.

## Unreleased

- IDW drawings saved by Inventor 2015 and 2016 (segment majors 19 and 20) and
  Inventor 2021 (segment major 25) are read as candidate profiles
  (`idw-rse31-meta8-major19-zlib-framing-candidate` and likewise for 20 and 25;
  typed fields `idw-majorNN-typed-fields-candidate`). They were rejected with
  `drawing.unsupported_profile`. Majors 19 and 20 use the major 23 layouts, like
  the other candidates from 15 to 22. Major 25 uses the major 26 layouts
  (sheet-link suffix, font table, RGBA view caches, triangle batches, the major 26
  extended records and the border, local display and filled-disk layouts),
  frames the `FBAttributeSegment` like major 24, and its view placements have no
  suffix GUID. Their displays are `experimental_partial`; nothing about them is
  qualified.
- Saved views of the candidate majors 15 to 22 (Inventor 2011 to 2018) decode.
  Their view placements use an older layout: 16-bit view flags, a 16-bit word
  between the two view reference lists and one (major 15) or two unresolved
  trailing bytes. The views, their RGBA caches and the model edges they reference
  are displayed; before, these drawings showed the sheet annotations only. New
  unresolved view fields: `view_list_separator_unresolved` and
  `view_tail_unresolved`; `view_flags_unresolved` is 16-bit on these majors.
  They also admit the border placement of majors 24, 26 and 28.
- Major 26, and the major 25 candidate, admit a historical target context in a
  display reference, as majors 23 and 30 do: only when the target's own ordered
  revision ranges contain it after the object's creation. Sheets that referenced
  such a context were unavailable (`display revision/object identity mismatch`).
- Large multi-sheet drawings no longer stop with `RSe aggregate work limit
  exceeded`. Container framing (`Limits.max_records`), sheet binding and display
  expansion (`DrawingLimits.max_reference_visits`) earn one allowance per started
  2 MiB of RSe stream bytes, at most four: 2,000,000 framing work items and
  12,000,000 reference visits with the default limits. Smaller inputs, including
  highly compressed ones, keep a single allowance, and lowering a limit lowers
  every allowance. The inventory's `usage.work_items` can therefore exceed
  `max_records`.
- A stroke override of width 0 keeps the inherited width and is flagged
  `stroke_override_width_zero_unverified`; it made the whole sheet display
  unavailable (`invalid stroke override width`). Other negative widths than the
  inherited -1 still fail closed.
