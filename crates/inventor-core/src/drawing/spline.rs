//! Observed major23 stored B-splines, not model regeneration or exact rendering.
use super::{fields::Fields, FieldValue, PayloadObservation};
use crate::{rse, Error, Result};

fn error(message: &str) -> Error {
    Error(message.into())
}

fn array(f: &mut Fields<'_, '_>, name: &'static str, dimension: usize) -> Result<()> {
    let count = f.r.count(65536)?;
    // All observed arrays store count == capacity, including empty weights.
    f.require(count as u32)?;
    f.require(8)?;
    f.doubles(name, count * dimension)
}

fn numbers<'a>(fields: &'a [super::FieldObservation], name: &str) -> Result<&'a [f64]> {
    let mut found = fields.iter().filter(|f| f.name == name);
    match (found.next(), found.next()) {
        (Some(f), None) => match &f.value {
            FieldValue::F64(values) => Ok(values),
            _ => Err(error("invalid spline numbers")),
        },
        _ => Err(error("missing/duplicate spline field")),
    }
}

pub(super) fn fields(f: &mut Fields<'_, '_>) -> Result<()> {
    f.display_header()?;
    let start = f.r.pos;
    rse::charge(f.work, 1)?;
    let flags = f.r.u32()?;
    if flags > 1 {
        return Err(error("unknown spline flags"));
    }
    f.add(
        "spline_flags_uninterpreted",
        start,
        FieldValue::U32(vec![flags]),
    );
    f.require(0)?;
    let start = f.r.pos;
    rse::charge(f.work, 1)?;
    let degree = f.r.u32()? as usize;
    if !(2..=4).contains(&degree) {
        return Err(error("unsupported stored spline degree"));
    }
    f.add("spline_degree", start, FieldValue::U32(vec![degree as u32]));
    f.doubles("spline_tolerance_candidate", 1)?;
    array(f, "spline_knots", 1)?;
    array(f, "spline_weights", 1)?;
    array(f, "spline_control_points", 3)?;
    f.doubles("spline_parameter_tolerance_candidate", 1)?;
    f.require(1)?;
    f.require(1)?;
    f.doubles("spline_parameter_range", 2)?;
    f.r.finish()?;
    if numbers(&f.fields, "spline_tolerance_candidate")? != [1e-9]
        || numbers(&f.fields, "spline_parameter_tolerance_candidate")? != [1e-12]
    {
        return Err(error("unknown spline tolerance layout"));
    }
    Spline::new(degree, &f.fields)?.validate()
}

pub(super) struct Spline<'a> {
    degree: usize,
    knots: &'a [f64],
    weights: &'a [f64],
    controls: &'a [f64],
    range: &'a [f64],
}
impl<'a> Spline<'a> {
    fn new(degree: usize, fields: &'a [super::FieldObservation]) -> Result<Self> {
        Ok(Self {
            degree,
            knots: numbers(fields, "spline_knots")?,
            weights: numbers(fields, "spline_weights")?,
            controls: numbers(fields, "spline_control_points")?,
            range: numbers(fields, "spline_parameter_range")?,
        })
    }
    pub fn observation(o: &'a PayloadObservation) -> Result<Self> {
        let degree = super::scene::word(o, "spline_degree")? as usize;
        let spline = Self::new(degree, &o.fields)?;
        spline.validate()?;
        Ok(spline)
    }
    fn validate(&self) -> Result<()> {
        let n = self.controls.len() / 3;
        if !(2..=4).contains(&self.degree)
            || !self.controls.len().is_multiple_of(3)
            || n < self.degree + 1
            || n > 65536
            || self.knots.len() != n + self.degree + 1
            || (!self.weights.is_empty() && self.weights.len() != n)
            || self.range.len() != 2
            || [self.knots, self.weights, self.controls, self.range]
                .into_iter()
                .flatten()
                .any(|v| !v.is_finite())
            || self.weights.iter().any(|v| *v <= 0.)
        {
            return Err(error("invalid stored spline arrays"));
        }
        let [start, end] = [self.knots[self.degree], self.knots[n]];
        if self.range[0] < start
            || self.range[1] > end
            || self.range[0] >= self.range[1]
            || !(end - start).is_finite()
            || self
                .knots
                .windows(2)
                .any(|p| p[0] > p[1] || !(p[1] - p[0]).is_finite())
        {
            return Err(error("invalid stored spline domain"));
        }
        let mut repeated = 1;
        for pair in self.knots.windows(2) {
            repeated = if pair[0] == pair[1] { repeated + 1 } else { 1 };
            // A polyline must not silently bridge a discontinuity.
            if pair[1] > start && pair[1] < end && repeated > self.degree {
                return Err(error("discontinuous stored spline unsupported"));
            }
        }
        Ok(())
    }
    pub fn spans(&self) -> impl Iterator<Item = (usize, f64, f64)> + '_ {
        (self.degree..self.controls.len() / 3).filter_map(|i| {
            let a = self.knots[i].max(self.range[0]);
            let b = self.knots[i + 1].min(self.range[1]);
            (a < b).then_some((i, a, b))
        })
    }
    pub fn stored_values(&self) -> usize {
        self.knots.len() + self.controls.len() + self.weights.len() + self.range.len()
    }
    /// Homogeneous de Boor evaluation. Positive weights prevent rational poles.
    /// Degree <= 4 bounds every evaluation's time and stack storage.
    pub fn evaluate(&self, span: usize, t: f64) -> Result<[f64; 3]> {
        let mut values = [[0.; 4]; 5];
        for (j, value) in values.iter_mut().enumerate().take(self.degree + 1) {
            let i = span - self.degree + j;
            let weight = self.weights.get(i).copied().unwrap_or(1.);
            for (k, v) in value.iter_mut().enumerate().take(3) {
                *v = self.controls[3 * i + k] * weight;
            }
            value[3] = weight;
        }
        for level in 1..=self.degree {
            for j in (level..=self.degree).rev() {
                let i = span - self.degree + j;
                let denominator = self.knots[i + self.degree + 1 - level] - self.knots[i];
                let alpha = (t - self.knots[i]) / denominator;
                if !denominator.is_finite()
                    || denominator <= 0.
                    || !alpha.is_finite()
                    || !(0. ..=1.).contains(&alpha)
                {
                    return Err(error("invalid spline evaluation interval"));
                }
                let previous = values[j - 1];
                for (value, prior) in values[j].iter_mut().zip(previous) {
                    *value = (1. - alpha) * prior + alpha * *value;
                }
            }
        }
        let value = values[self.degree];
        let result = [
            value[0] / value[3],
            value[1] / value[3],
            value[2] / value[3],
        ];
        if value[3] <= 0. || !value[3].is_finite() || result.iter().any(|v| !v.is_finite()) {
            return Err(error("non-finite rational spline evaluation"));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::SourceSpan;

    fn wire(weights: &[f64]) -> Vec<u8> {
        let mut b = vec![0; 26];
        for v in [1u32, 0, 2] {
            b.extend(v.to_le_bytes());
        }
        b.extend(1e-9f64.to_le_bytes());
        for (values, dimension) in [
            (&[0., 0., 0., 1., 1., 1.][..], 1),
            (weights, 1),
            (&[1., 0., 0., 1., 1., 0., 0., 1., 0.][..], 3),
        ] {
            let n = (values.len() / dimension) as u32;
            for v in [n, n, 8] {
                b.extend(v.to_le_bytes());
            }
            for v in values {
                b.extend(v.to_le_bytes());
            }
        }
        b.extend(1e-12f64.to_le_bytes());
        for v in [1u32, 1] {
            b.extend(v.to_le_bytes());
        }
        for v in [0f64, 1.] {
            b.extend(v.to_le_bytes());
        }
        b
    }
    fn parse(b: &[u8], major: u8, work: &mut usize) -> Result<Option<PayloadObservation>> {
        super::super::fields::decode(
            "DlSheetDlSegmentType",
            major,
            "d3a55702-11d1-ebbb-62ae-0297584063da",
            0,
            b,
            SourceSpan::stream("synthetic", "/B", 0, b.len()),
            work,
        )
    }
    #[test]
    fn rational_quarter_circle_and_wire_rejections() {
        let b = wire(&[1., std::f64::consts::FRAC_1_SQRT_2, 1.]);
        let o = parse(&b, 23, &mut 1000).unwrap().unwrap();
        let spline = Spline::observation(&o).unwrap();
        let (span, _, _) = spline.spans().next().unwrap();
        assert_eq!(spline.evaluate(span, 0.).unwrap(), [1., 0., 0.]);
        assert_eq!(spline.evaluate(span, 1.).unwrap(), [0., 1., 0.]);
        let p = spline.evaluate(span, 0.5).unwrap();
        for x in p.iter().take(2) {
            assert!((x - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-14);
        }
        for i in 0..=32 {
            let p = spline.evaluate(span, i as f64 / 32.).unwrap();
            assert!((p[0] * p[0] + p[1] * p[1] - 1.).abs() < 1e-14);
        }
        assert!(parse(&b, 31, &mut 1000).unwrap().is_none());
        assert!(parse(&b, 23, &mut 4).is_err());
        for end in 0..b.len() {
            assert!(parse(&b[..end], 23, &mut 1000).is_err());
        }
        let mut extra = b.clone();
        extra.push(0);
        assert!(parse(&extra, 23, &mut 1000).is_err());
        for (offset, value) in [
            (26, 2u32),
            (30, 1),
            (34, 5),
            (46, u32::MAX),
            (50, 7),
            (54, 4),
        ] {
            let mut bad = b.clone();
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(parse(&bad, 23, &mut 1000).is_err(), "offset {offset}");
        }
        for weights in [
            &[1., 0., 1.][..],
            &[1., -1., 1.],
            &[1., f64::NAN, 1.],
            &[1., 1.],
        ] {
            assert!(parse(&wire(weights), 23, &mut 1000).is_err());
        }
        for offset in [38, 58, b.len() - 32, b.len() - 16, b.len() - 8] {
            let mut bad = b.clone();
            bad[offset..offset + 8].copy_from_slice(&f64::INFINITY.to_le_bytes());
            assert!(parse(&bad, 23, &mut 1000).is_err());
        }
        // Each typed source span is still the original little-endian bytes.
        for f in &o.fields {
            if let FieldValue::F64(values) = &f.value {
                let bytes: Vec<_> = values.iter().flat_map(|x| x.to_le_bytes()).collect();
                assert_eq!(bytes, &b[f.source.start_offset..f.source.end_offset]);
            }
        }
    }
    #[test]
    fn polynomial_degrees_trim_and_repeated_knots() {
        for (degree, controls, expected) in [
            (2, vec![0., 0., 0., 1., 2., 0., 2., 0., 0.], [1., 1., 0.]),
            (
                3,
                vec![0., 0., 0., 0., 1., 0., 1., 1., 0., 1., 0., 0.],
                [0.5, 0.75, 0.],
            ),
            (
                4,
                vec![
                    0., 0., 0., 0.25, 0., 0., 0.5, 1., 0., 0.75, 0., 0., 1., 0., 0.,
                ],
                [0.5, 0.375, 0.],
            ),
        ] {
            let mut knots = vec![0.; degree + 1];
            knots.extend(vec![1.; degree + 1]);
            let s = Spline {
                degree,
                knots: &knots,
                controls: &controls,
                weights: &[],
                range: &[0.25, 0.75],
            };
            s.validate().unwrap();
            assert_eq!(s.spans().collect::<Vec<_>>(), vec![(degree, 0.25, 0.75)]);
            assert_eq!(s.evaluate(degree, 0.5).unwrap(), expected);
        }
        let s = Spline {
            degree: 2,
            knots: &[0., 0., 0., 0.5, 0.5, 1., 1., 1.],
            weights: &[],
            controls: &[
                0., 0., 0., 0.25, 1., 0., 0.5, 0., 0., 0.75, -1., 0., 1., 0., 0.,
            ],
            range: &[0.125, 0.875],
        };
        s.validate().unwrap();
        assert_eq!(
            s.spans().collect::<Vec<_>>(),
            vec![(2, 0.125, 0.5), (4, 0.5, 0.875)]
        );
        assert_eq!(s.evaluate(2, 0.25).unwrap(), [0.25, 0.5, 0.]);
        assert_eq!(s.evaluate(4, 0.75).unwrap(), [0.75, -0.5, 0.]);
        assert_eq!(s.evaluate(2, 0.5).unwrap(), s.evaluate(4, 0.5).unwrap());
        assert!(Spline {
            range: &[-0.1, 1.],
            ..s
        }
        .validate()
        .is_err());
        assert!(Spline {
            range: &[0.5, 0.5],
            ..s
        }
        .validate()
        .is_err());
        assert!(Spline {
            knots: &[0., 0., 0., 0.5, 0.4, 1., 1., 1.],
            ..s
        }
        .validate()
        .is_err());
        assert!(Spline {
            knots: &[0., 0., 0., 0.5, 0.5, 0.5, 1., 1., 1.],
            controls: &[0.; 18],
            ..s
        }
        .validate()
        .is_err());
    }
}
