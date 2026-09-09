use crate::{Error, Result};
pub type Matrix = [[f64; 4]; 4];
pub const IDENTITY: Matrix = [
    [1., 0., 0., 0.],
    [0., 1., 0., 0.],
    [0., 0., 1., 0.],
    [0., 0., 0., 1.],
];

pub(crate) fn affine(m: &Matrix) -> Result<()> {
    if m.iter().flatten().any(|x| !x.is_finite()) || m[3] != [0., 0., 0., 1.] {
        return Err(Error("placement is not a finite affine matrix".into()));
    }
    Ok(())
}

/// Row-major matrices acting on column vectors: world = parent * local.
pub fn compose(parent: &Matrix, local: &Matrix) -> Result<Matrix> {
    affine(parent)?;
    affine(local)?;
    let mut result = [[0.; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            result[i][j] = (0..4).map(|k| parent[i][k] * local[k][j]).sum();
        }
    }
    affine(&result)?;
    Ok(result)
}
