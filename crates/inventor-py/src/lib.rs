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
#[cfg(feature = "analytic-writer")]
#[pyfunction]
fn encode_analytic_part<'py>(
    py: Python<'py>,
    specification: &str,
) -> PyResult<Bound<'py, PyBytes>> {
    if specification.len() > 64 * 1024 * 1024 {
        return Err(PyValueError::new_err(
            "semantic writer JSON byte budget exceeded",
        ));
    }
    let bytes = py.detach(|| {
        let part: inventor_core::write::analytic::PartDocument =
            serde_json::from_str(specification)
                .map_err(|e| PyValueError::new_err(format!("invalid semantic part: {e}")))?;
        part.encode()
            .map_err(|e| PyValueError::new_err(e.to_string()))
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[cfg(feature = "iam-writer")]
#[pyfunction]
fn inspect_authored_part(data: &Bound<'_, PyBytes>, filename: &str) -> PyResult<String> {
    let bytes = data.as_bytes();
    let part = data
        .py()
        .detach(|| {
            inventor_core::write::assembly::PartDefinition::from_authored_ipt(filename, bytes)
        })
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    serde_json::to_string(&part).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[cfg(feature = "iam-writer")]
#[pyfunction]
fn encode_fixed_assembly<'py>(
    py: Python<'py>,
    specification: &str,
    parts: Vec<(String, Bound<'py, PyBytes>)>,
) -> PyResult<(Bound<'py, PyBytes>, String)> {
    use inventor_core::write::assembly::{AssemblyDocument, PartDefinition};
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Occurrence {
        part: usize,
        name: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Spec {
        name: String,
        target_year: Option<u16>,
        part_number: String,
        occurrences: Vec<Occurrence>,
    }
    if specification.len() > 65536
        || parts.is_empty()
        || parts.len() > 32
        || parts
            .iter()
            .any(|(_, b)| b.as_bytes().len() > 64 * 1024 * 1024)
        || parts.iter().map(|(_, b)| b.as_bytes().len()).sum::<usize>() > 256 * 1024 * 1024
    {
        return Err(PyValueError::new_err("IAM input budget exceeded"));
    }
    let inputs: Vec<_> = parts
        .iter()
        .map(|(name, b)| (name.as_str(), b.as_bytes()))
        .collect();
    let (bytes, report) = py
        .detach(|| -> Result<_, String> {
            let spec: Spec = serde_json::from_str(specification).map_err(|e| e.to_string())?;
            if spec.occurrences.is_empty() || spec.occurrences.len() > 32 {
                return Err("expected 1..32 occurrences".into());
            }
            let mut definitions = Vec::new();
            for (name, b) in &inputs {
                definitions
                    .push(PartDefinition::from_authored_ipt(name, b).map_err(|e| e.to_string())?);
            }
            let target = match spec.target_year.unwrap_or(2027) {
                2026 => inventor_core::write::TargetVersion::Inventor2026,
                2027 => inventor_core::write::TargetVersion::Inventor2027,
                _ => return Err("unsupported target year".into()),
            };
            let mut doc = AssemblyDocument::empty(&spec.name)
                .and_then(|d| d.with_target(target))
                .and_then(|d| d.with_part_number(&spec.part_number))
                .map_err(|e| e.to_string())?;
            let mut used = std::collections::BTreeSet::new();
            for o in spec.occurrences {
                used.insert(o.part);
                let part = definitions
                    .get(o.part)
                    .ok_or("invalid occurrence part index")?;
                doc.add_part(part.clone(), &o.name)
                    .map_err(|e| e.to_string())?;
            }
            if used.len() != inputs.len() {
                return Err("unused part input".into());
            }
            let prepared = doc.prepare();
            let bytes = prepared.encode().map_err(|e| e.to_string())?;
            let report = serde_json::to_string(&prepared).map_err(|e| e.to_string())?;
            Ok((bytes, report))
        })
        .map_err(PyValueError::new_err)?;
    Ok((PyBytes::new(py, &bytes), report))
}

#[pymodule]
fn _inventor(module: &Bound<'_, PyModule>) -> PyResult<()> {
    #[cfg(feature = "analytic-writer")]
    module.add_function(wrap_pyfunction!(encode_analytic_part, module)?)?;
    #[cfg(feature = "iam-writer")]
    {
        module.add_function(wrap_pyfunction!(inspect_authored_part, module)?)?;
        module.add_function(wrap_pyfunction!(encode_fixed_assembly, module)?)?;
    }
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
