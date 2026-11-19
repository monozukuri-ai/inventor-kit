# Changelog

Release notes of published versions are in [docs](docs/README.md)
(`release-notes-vX.Y.Z.md`). This file collects changes that are not released yet.

## Unreleased

- Hole notes, sets of chained and ordinate dimensions, notes with leaders,
  frames and tables that a sheet stores as annotation nodes of its own are
  displayed. Each owns one local display and frames with the shared local or
  leader display layout, but its node type was not in the decoder table, so
  the node was an omission (`sheet_node_not_decoded`) and its texts, leaders
  and dimension lines were not reached
  (`not_reached_from_stored_sheet_bindings`): a part drawing lost the values
  of its chained dimensions and its hole notes while the other dimensions were
  shown. The hole note and the chained set are pinned by a public major 26
  part drawing (a new regression fixture); the other node types were seen on
  production drawings and are admitted on the generations that store them
  (majors 11 to 14 and 15 to 22 each as a block). The local annotation
  displays of majors 25, 26 and 28 and the local display record of major 23
  now frame on every generation, the leader display of majors 26 and 28 also
  on majors 23, 24, 29 and 31, the balloon display also on majors 24, 29 and
  31, and the external display of the extended records also on majors 24, 28,
  29 and 31. Two of the major 25 and 26 revision, section and detail nodes
  also frame on the other generations that store them. One node type whose
  display reference is not always a local slot stays an omission.
- A display reference may name a historical target context on every profile.
  Majors 24, 27, 28, 29 and 31 required the target's latest revision, so a
  sheet whose saved display was not rewritten by the document's last save was
  `unavailable` (`display revision/object identity mismatch`) although its
  display space decoded (`display_space_not_in_document_sheet_list`). Seen on
  production drawings of majors 24, 27, 29 and 31. As before on majors 23, 25,
  26 and 30, the context is admitted only inside the target's own ordered
  revision ranges, after the object's creation; any other context reports
  `missing or pre-creation target context`.
- A display group with a cross-segment child (an untagged reference) is omitted
  on every profile (`cross_segment_display_children_not_supported`), as on
  major 23. On the other majors one such child made the sheet `unavailable`
  (`unsupported local display reference tag`); seen on a production major 29
  drawing.
- The expanded bytes of a drawing's bulk streams earn the input-size
  allowances of the work budgets: `Limits.max_inflated_bytes` (one bulk
  stream) and `Limits.max_total_inflated_bytes` (the document) are multiplied
  by the allowances, at most four (256 MiB and 512 MiB with the defaults), in
  the inventory and when saved view caches are read again for their images. A
  sheet whose view caches expand past 64 MiB stopped with `decompression byte
  limit exceeded`. Meta streams, parts and assemblies keep the unscaled limits.
- An allowance is earned per started MiB of RSe stream bytes (it was 2 MiB);
  the maximum stays four. A sheet of dense line work within `max_display_items`
  ran out of reference visits (`RSe aggregate work limit exceeded`).
- A saved view whose bitmap cache has the empty box as its bounds (minimum
  `+DBL_MAX`, maximum `-DBL_MAX`) no longer makes its sheet `unavailable`
  (`invalid view bitmap bounds`). The view is kept without its image and
  flagged `view_cache_bounds_empty`, and the cache record is listed in the
  sheet's omissions with the same reason. Since saved views of major 30 decode,
  one such view discarded the border, the other views and the annotations of
  its sheet. Other inverted or zero-sized bounds are still rejected.
- IDW drawings saved by Inventor 11 and 2008 to 2010 (segment majors 11 to 14)
  are read as candidate profiles
  (`idw-rse29-meta6-major11-zlib-framing-candidate`,
  `idw-rse30-meta7-major12-zlib-framing-candidate` and likewise for 13,
  `idw-rse31-meta7-major14-zlib-framing-candidate`; typed fields
  `idw-majorNN-typed-fields-candidate`). Major 14 was rejected with
  `drawing.unsupported_profile`, majors 11 to 13 already at their RSeDb schema
  (`drawing.ambiguous_owner`). They store the major 15 layouts, but every class
  layer and nested section of a record ends with a u32 marker holding its byte
  length, and the Meta block table lists the same values after each record's
  length word. Records are therefore numbered without those words, and each
  decoded record must close exactly the listed sections. Sheets, saved views
  (RGBA caches where stored), annotations (including the local annotation and
  leader displays that majors 15 to 22 now frame, below), triangle batches with
  index flags 0 and border placements are displayed, and color mask 8 is read
  as on the other majors (below); sheet definitions keep the prefix-only
  observation. Observed per-major differences are listed in
  [the drawing guide](docs/drawing.md#observed-segment-profiles). Point markers of
  these majors keep two floats in `marker_parameters_unresolved`, a short (a
  byte on major 11) in `marker_suffix_unresolved` and the new
  `marker_layer_tail_unresolved`. Their displays are `experimental_partial`;
  nothing about them is qualified.
- RSeDb schemas 29 and 30 are admitted for drawings only. Their registry keeps
  the schema 31 entries but ends with two lists of class names, and schema 29
  node records are 18 bytes. Meta stream version 6, which stores a fourth state
  word and a 32-bit time value in its header, is read. Parts and assemblies
  still require schema 31. Schemas 26 and 21 (no RSeSegInfo stream, Meta stream
  versions 5 and 4) remain unsupported.
- Each drawing profile admits only the Meta stream version it was observed with
  (6 for major 11, 7 for majors 12 to 14, 8 from major 15); a mismatch fails
  before decompression with `drawing.meta_framing_unavailable`. Version 7
  streams were also read for majors 15 to 31 before.
- In Meta6/7 block tables `Limits.max_records` bounds the records, not the
  words, and framing charges the layer markers once.
- `scripts/check_legacy_drawings.py` rechecks a local directory of private
  major 11 to 14 drawings (`--input` or `INVENTOR_KIT_LEGACY_DRAWINGS`) and
  reports structural counts only.
- Saved views of IDW drawings saved by Inventor 2026 (segment major 30) decode.
  Their view placements store two reference lists before the view name, like
  majors 26 to 29; the candidate profile expected the third list of major 31,
  so every view placement failed to frame and its model edges were not
  displayed (`not_reached_from_stored_sheet_bindings`). Major 30 also frames
  the splines, ellipses, filled SM disks and the local, leader, sketch, table,
  border and external annotation placements with the layouts of majors 25 to
  28; they were omitted (`display_type_not_decoded`, `sheet_node_not_decoded`).
  The display stays `experimental_partial`.
- Display color mask 8 outside major 24 is read like the native-matched
  major 24 override and flagged `display_color_mask8_unverified`, as
  `docs/drawing.md` describes; the element and its subtree were omitted
  (`display_color_mask_not_interpreted`). Other masks still omit their subtree.
  The pinned major 28 regression drawing displays one more element (1,651).
- The candidate majors 15 to 22 and 27 frame the local annotation displays of
  majors 25, 26 and 28, majors 15 to 22 also the leader display of majors 26
  and 28, and major 27 the local display of majors 23 and 25 and the border
  placement. Their annotations were omitted (`sheet_node_not_decoded`).
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
- Typed-field parsing (`DrawingLimits.max_field_values`) earns the same
  allowances: one per started 2 MiB of RSe stream bytes, at most four
  (4,000,000 typed field values with the default limit). Drawings whose typed
  fields needed more than one allowance lost the remaining record observations
  (`drawing.fields_unavailable`, `RSe aggregate work limit exceeded`) and could
  end without a display. Smaller inputs keep a single allowance, lowering the
  limit lowers every allowance, and the inventory's `usage.field_work_items` can
  therefore exceed `max_field_values`.
- A stroke override of width 0 keeps the inherited width and is flagged
  `stroke_override_width_zero_unverified`; it made the whole sheet display
  unavailable (`invalid stroke override width`). Other negative widths than the
  inherited -1 still fail closed.
