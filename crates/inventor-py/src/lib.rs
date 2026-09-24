use pyo3::{exceptions::PyValueError, prelude::*, types::PyBytes};

mod drawing_wire;

type ReadResult = (String, Option<Py<PyAny>>, Option<Py<PyBytes>>);

fn read_limits(json: Option<&str>) -> PyResult<inventor_core::Limits> {
    let limits: inventor_core::Limits = json
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| PyValueError::new_err(format!("invalid limits: {e}")))?
        .unwrap_or_default();
    limits
        .validate()
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(limits)
}

#[pyfunction]
fn default_limits() -> PyResult<String> {
    serde_json::to_string(&inventor_core::Limits::default())
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
#[pyo3(signature = (data, source_id, limits_json=None))]
fn inspect_assembly(
    data: &Bound<'_, PyBytes>,
    source_id: &str,
    limits_json: Option<&str>,
) -> PyResult<String> {
    let limits = read_limits(limits_json)?;
    if data.as_bytes().len() > limits.max_file_bytes {
        return Err(PyValueError::new_err("file byte limit exceeded"));
    }
    // Immutable PyBytes stays owned throughout synchronous detach; borrow its
    // buffer instead of copying up to 128 MiB on every call.
    let bytes = data.as_bytes();
    let doc = data
        .py()
        .detach(|| inventor_core::assembly::inspect(bytes, source_id, &limits))
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    serde_json::to_string(&doc).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn default_drawing_limits() -> PyResult<String> {
    serde_json::to_string(&inventor_core::drawing::DrawingLimits::default())
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
#[pyo3(signature = (data, source_id, limits_json=None, drawing_limits_json=None))]
fn read_drawing(
    data: &Bound<'_, PyBytes>,
    source_id: &str,
    limits_json: Option<&str>,
    drawing_limits_json: Option<&str>,
) -> PyResult<String> {
    use inventor_core::drawing;
    let limits = read_limits(limits_json)?;
    let drawing_limits: drawing::DrawingLimits = drawing_limits_json
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| PyValueError::new_err(format!("invalid drawing limits: {e}")))?
        .unwrap_or_default();
    drawing_limits
        .validate()
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    if data.as_bytes().len() > limits.max_file_bytes {
        return Err(PyValueError::new_err("file byte limit exceeded"));
    }
    let bytes = data.as_bytes();
    data.py().detach(|| {
        let inventory = drawing::inspect_with_limits(bytes, source_id, &limits, &drawing_limits)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        if inventory.metadata.identification.kind != "drawing" {
            return Err(PyValueError::new_err("identified IDW document required"));
        }
        let sheets = drawing::stored_sheets_with_limits(&inventory, &limits, &drawing_limits);
        let preview = drawing::experimental_scene_with_limits(&inventory, &limits, &drawing_limits);
        let images =
            drawing::read_embedded_images_with_limits(bytes, &preview, &limits, &drawing_limits)
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let diagnostics: Vec<_> = inventory
            .diagnostics
            .iter()
            .chain(inventory.segments.iter().flat_map(|s| s.diagnostics.iter()))
            .collect();
        #[derive(serde::Serialize)]
        struct Output<'a> {
            api_version: u32,
            wire_version: u32,
            source_sha256: &'a str,
            metadata: &'a inventor_core::document::DocumentInfo,
            sheets: &'a drawing::StoredSheets,
            preview: drawing_wire::Scene<'a>,
            images: &'a Vec<drawing::EmbeddedImage>,
            diagnostics: Vec<&'a inventor_core::document::Diagnostic>,
        }
        let output = Output {
            api_version: 1,
            wire_version: 2,
            source_sha256: &inventory.source_sha256,
            metadata: &inventory.metadata,
            sheets: &sheets,
            preview: drawing_wire::Scene::new(&preview, &drawing_limits)
                .map_err(PyValueError::new_err)?,
            images: &images,
            diagnostics,
        };
        let mut writer = drawing_limits
            .output_buffer()
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        serde_json::to_writer(&mut writer, &output)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        writer
            .into_string()
            .map_err(|e| PyValueError::new_err(e.to_string()))
    })
}

#[pyfunction]
#[pyo3(signature = (path, search_roots, max_documents=256, max_instances=10000, max_depth=32,
    max_total_file_bytes=536870912, max_directory_entries=100000, limits_json=None))]
#[allow(clippy::too_many_arguments)] // Explicit independently bounded Python options.
fn resolve_assembly(
    py: Python<'_>,
    path: String,
    search_roots: Vec<String>,
    max_documents: usize,
    max_instances: usize,
    max_depth: usize,
    max_total_file_bytes: usize,
    max_directory_entries: usize,
    limits_json: Option<&str>,
) -> PyResult<String> {
    let limits = read_limits(limits_json)?;
    let options = inventor_core::assembly::ResolveOptions {
        search_roots: search_roots.into_iter().map(Into::into).collect(),
        max_documents,
        max_instances,
        max_depth,
        max_total_file_bytes,
        max_directory_entries,
    };
    let graph = py
        .detach(|| {
            inventor_core::assembly::resolve_file(std::path::Path::new(&path), &limits, &options)
        })
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    serde_json::to_string(&graph).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
#[pyo3(signature = (data, source_id, include_candidates=false, limits_json=None))]
fn inspect(
    data: &Bound<'_, PyBytes>,
    source_id: &str,
    include_candidates: bool,
    limits_json: Option<&str>,
) -> PyResult<String> {
    let limits = read_limits(limits_json)?;
    if data.as_bytes().len() > limits.max_file_bytes {
        return Err(PyValueError::new_err("file byte limit exceeded"));
    }
    let bytes = data.as_bytes();
    let doc = data
        .py()
        .detach(|| {
            if include_candidates {
                inventor_core::inspect_candidates(bytes, source_id, &limits)
            } else {
                inventor_core::inspect(bytes, source_id, &limits)
            }
        })
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    serde_json::to_string(&doc.summary).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
#[pyo3(signature = (data, source_id, candidate_id=None, require_current_state=false, limits_json=None))]
fn read<'py>(
    data: &Bound<'py, PyBytes>,
    source_id: &str,
    candidate_id: Option<String>,
    require_current_state: bool,
    limits_json: Option<&str>,
) -> PyResult<ReadResult> {
    let py = data.py();
    let limits = read_limits(limits_json)?;
    if data.as_bytes().len() > limits.max_file_bytes {
        return Err(PyValueError::new_err("file byte limit exceeded"));
    }
    let bytes = data.as_bytes();
    let options = inventor_core::candidate::ReadOptions {
        candidate_id,
        require_current_state,
    };
    let doc = py
        .detach(|| inventor_core::read_with_options(bytes, source_id, &limits, &options))
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let summary =
        serde_json::to_string(&doc.summary).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let model = doc
        .model
        .map(|m| acis_py_bridge::model_to_python(py, m).map(Bound::unbind))
        .transpose()?;
    Ok((
        summary,
        model,
        doc.kernel_bytes.map(|b| PyBytes::new(py, &b).unbind()),
    ))
}
#[pymodule]
fn _inventor(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(default_limits, module)?)?;
    module.add_function(wrap_pyfunction!(default_drawing_limits, module)?)?;
    module.add_function(wrap_pyfunction!(read, module)?)?;
    module.add_function(wrap_pyfunction!(inspect, module)?)?;
    module.add_function(wrap_pyfunction!(inspect_assembly, module)?)?;
    module.add_function(wrap_pyfunction!(read_drawing, module)?)?;
    module.add_function(wrap_pyfunction!(resolve_assembly, module)?)?;
    module.add("MODEL_API_VERSION", acis_py_bridge::MODEL_API_VERSION)?;
    module.add("DOCUMENT_API_VERSION", 1)?;
    module.add("GEOMETRY_INVENTORY_API_VERSION", 1)?;
    module.add("ASSEMBLY_API_VERSION", 1)?;
    module.add("DRAWING_API_VERSION", 1)?;
    Ok(())
}
