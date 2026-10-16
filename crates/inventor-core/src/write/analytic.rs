//! Bounded semantic single-solid authoring for explicit Inventor targets.
//! No file, template, SAB or record profile is accepted as input.
use super::*;
use acis_core::authoring::brep as b;
use serde::Deserialize;
mod graphics;
pub use graphics::{Display, EdgeDisplay, FaceDisplay};
pub const PROFILE_2026: &str = "inventor-ipt-rse31-major30-metric-analytic-base-feature-v1";
pub const PROFILE: &str = "inventor-ipt-rse31-metric-analytic-base-feature-v21";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub document_name: String,
    pub feature_name: String,
    pub body_name: String,
    pub title: String,
    pub part_number: String,
}
impl Metadata {
    fn validate(&self) -> Result<()> {
        for (s, empty) in [
            (&self.document_name, false),
            (&self.feature_name, false),
            (&self.body_name, false),
            (&self.title, true),
            (&self.part_number, true),
        ] {
            if (!empty && s.is_empty()) || s.contains('\0') || s.encode_utf16().count() > 255 {
                return Err(Error("metadata must be at most 255 UTF-16 units, without NUL; names must be nonempty".into()));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Curve {
    Line {
        origin: [f64; 3],
        direction: [f64; 3],
    },
    Ellipse {
        center: [f64; 3],
        normal: [f64; 3],
        major_axis: [f64; 3],
        ratio: f64,
    },
}
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Surface {
    Plane {
        origin: [f64; 3],
        normal: [f64; 3],
        u_direction: [f64; 3],
    },
    Cylinder {
        center: [f64; 3],
        axis: [f64; 3],
        x_direction: [f64; 3],
        radius: f64,
    },
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub vertices: [usize; 2],
    pub parameters: [f64; 2],
    pub curve: Curve,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Face {
    pub surface: Surface,
    pub reversed: bool,
    pub loops: Vec<Vec<(usize, bool)>>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Solid {
    pub vertices_mm: Vec<[f64; 3]>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    pub tolerance_mm: f64,
}
fn v(a: [f64; 3]) -> acis_core::Vec3 {
    (a[0], a[1], a[2]).into()
}
impl Solid {
    fn build(&self) -> Result<b::AuthoredBrep> {
        if self.faces.is_empty()
            || self.edges.is_empty()
            || self.faces.len() + self.edges.len() > 512
            || self.vertices_mm.len() > 4096
            || !self.tolerance_mm.is_finite()
            || self.tolerance_mm <= 0.0
            || self.tolerance_mm > 1e-5
        {
            return Err(Error("analytic topology/tolerance budget exceeded".into()));
        }
        b::BrepSpec {
            vertices_mm: self.vertices_mm.iter().copied().map(v).collect(),
            tolerance_mm: self.tolerance_mm,
            edges: self
                .edges
                .iter()
                .map(|e| b::EdgeSpec {
                    vertices: e.vertices,
                    parameters: e.parameters,
                    curve: match e.curve {
                        Curve::Line { origin, direction } => b::CurveSpec::Line {
                            origin: v(origin),
                            direction: v(direction),
                        },
                        Curve::Ellipse {
                            center,
                            normal,
                            major_axis,
                            ratio,
                        } => b::CurveSpec::Ellipse {
                            center: v(center),
                            normal: v(normal),
                            major_axis: v(major_axis),
                            ratio,
                        },
                    },
                })
                .collect(),
            faces: self
                .faces
                .iter()
                .map(|f| b::FaceSpec {
                    reversed: f.reversed,
                    loops: f
                        .loops
                        .iter()
                        .map(|l| {
                            l.iter()
                                .map(|&(edge, reversed)| b::EdgeUse { edge, reversed })
                                .collect()
                        })
                        .collect(),
                    surface: match f.surface {
                        Surface::Plane {
                            origin,
                            normal,
                            u_direction,
                        } => b::SurfaceSpec::Plane {
                            origin: v(origin),
                            normal: v(normal),
                            u_direction: v(u_direction),
                        },
                        Surface::Cylinder {
                            center,
                            axis,
                            x_direction,
                            radius,
                        } => b::SurfaceSpec::Cylinder {
                            center: v(center),
                            axis: v(axis),
                            x_direction: v(x_direction),
                            radius,
                        },
                    },
                })
                .collect(),
        }
        .build()
        .map_err(err)
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartDocument {
    pub profile: String,
    pub solid: Solid,
    pub display: Display,
    pub metadata: Metadata,
}
impl PartDocument {
    pub fn encode(&self) -> Result<Vec<u8>> {
        let target = match self.profile.as_str() {
            PROFILE => TargetVersion::Inventor2027,
            PROFILE_2026 => TargetVersion::Inventor2026,
            _ => return Err(Error("unsupported analytic writer profile".into())),
        };
        self.metadata.validate()?;
        let authored = self.solid.build()?;
        self.display.validate(&self.solid)?;
        let profile = graphics::construct(&self.solid, &self.display, target)?;
        let expected = array(&profile.0, "records")?.len();
        let bounds = self.display.bounds_mm;
        // Workplanes and standard document scaffolding use the enclosing box.
        // Both exact BRep carriers and all body display records use this solid.
        let mut input = BoxDocument::new(BoxSpec::new(
            (
                bounds[3] - bounds[0],
                bounds[4] - bounds[1],
                bounds[5] - bounds[2],
            )
                .into(),
            (bounds[0], bounds[1], bounds[2]).into(),
        ));
        input.document_name = self.metadata.document_name.clone();
        input.feature_name = self.metadata.feature_name.clone();
        input.body_name = self.metadata.body_name.clone();
        let bbox = input.shape.build().map_err(err)?;
        let mut graph = document::Graph::new(profile, &input, bbox)?;
        let (source, labels) = kernel_labels::encode_analytic(
            &authored,
            &self.solid,
            kernel_labels::History::Source,
            target,
        )?;
        let (result, other) = kernel_labels::encode_analytic(
            &authored,
            &self.solid,
            kernel_labels::History::BaseFeature,
            target,
        )?;
        if labels != other {
            return Err(Error("source/result label census differs".into()));
        }
        graph.kernels = Some([source, result]);
        graph.label_count = Some(labels);
        graph.metadata = Some((
            self.metadata.title.clone(),
            self.metadata.part_number.clone(),
        ));
        let bytes = cfb::encode(rse::streams(&graph)?)?;
        if bytes.len() > 128 * 1024 * 1024 {
            return Err(Error("IPT byte budget exceeded".into()));
        }
        let read = crate::read(&bytes, "authored-analytic", &crate::Limits::default())?;
        if read.summary.kind != "part" || read.model.is_none() {
            return Err(Error("analytic self-read failed".into()));
        }
        let inventory =
            crate::part::inspect(&bytes, "authored-analytic", &crate::Limits::default())?;
        if inventory.segments.len() != 8
            || inventory
                .segments
                .iter()
                .map(|s| s.records.len())
                .sum::<usize>()
                != expected
        {
            return Err(Error("analytic self-read census differs".into()));
        }
        Ok(bytes)
    }
}
