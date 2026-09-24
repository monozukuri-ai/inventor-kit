//! Private Python transport: share identical styles without dropping provenance.
use inventor_core::{document::SourceSpan, drawing};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub(super) struct Scene<'a> {
    status: &'static str,
    qualified: bool,
    source_sha256: &'a str,
    units: &'static str,
    spaces: Vec<Space<'a>>,
    diagnostics: &'a [String],
}

#[derive(Serialize)]
struct Space<'a> {
    segment_id: &'a str,
    extent_candidate: [f64; 2],
    bindings: &'a [drawing::DisplayBinding],
    views: &'a [drawing::StoredView],
    items: Vec<Item<'a>>,
    styles: Vec<&'a drawing::DisplayStyle>,
    omitted: &'a [drawing::Omission],
}

#[derive(Serialize)]
struct Item<'a> {
    segment_id: &'a str,
    record_ordinal: usize,
    source: &'a SourceSpan,
    placement_record: usize,
    group_path: &'a [usize],
    transform: &'a [[f64; 4]; 4],
    geometry: &'a drawing::DisplayGeometry,
    style_index: usize,
}

impl<'a> Scene<'a> {
    pub(super) fn new(
        scene: &'a drawing::ExperimentalScene,
        limits: &drawing::DrawingLimits,
    ) -> Result<Self, String> {
        let mut spaces = Vec::with_capacity(scene.spaces.len());
        let mut key_bytes = 0usize;
        for space in &scene.spaces {
            let mut indexes = BTreeMap::new();
            let mut styles = Vec::new();
            let mut items = Vec::with_capacity(space.items.len());
            for item in &space.items {
                // Bound both each canonical key and the retained index keys.
                // The final payload still goes through the same output ceiling.
                let mut writer = limits.output_buffer().map_err(|e| e.to_string())?;
                serde_json::to_writer(&mut writer, &item.style).map_err(|e| e.to_string())?;
                let key = writer.into_string().map_err(|e| e.to_string())?;
                let style_index = if let Some(index) = indexes.get(&key) {
                    *index
                } else {
                    key_bytes = key_bytes
                        .checked_add(key.len())
                        .filter(|n| *n <= limits.max_output_bytes)
                        .ok_or("drawing output byte limit exceeded")?;
                    let index = styles.len();
                    indexes.insert(key, index);
                    styles.push(&item.style);
                    index
                };
                items.push(Item {
                    segment_id: &item.segment_id,
                    record_ordinal: item.record_ordinal,
                    source: &item.source,
                    placement_record: item.placement_record,
                    group_path: &item.group_path,
                    transform: &item.transform,
                    geometry: &item.geometry,
                    style_index,
                });
            }
            spaces.push(Space {
                segment_id: &space.segment_id,
                extent_candidate: space.extent_candidate,
                bindings: &space.bindings,
                views: &space.views,
                items,
                styles,
                omitted: &space.omitted,
            });
        }
        Ok(Self {
            status: scene.status,
            qualified: scene.qualified,
            source_sha256: &scene.source_sha256,
            units: scene.units,
            spaces,
            diagnostics: &scene.diagnostics,
        })
    }
}
