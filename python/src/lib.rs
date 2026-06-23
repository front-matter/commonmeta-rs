//! PyO3 bindings exposing commonmeta-rs's `list`-command batch functions to
//! Python. Records cross the FFI boundary as plain JSON-shaped dicts (via
//! `pythonize`), matching the shape commonmeta-py's own readers/writers
//! already produce/consume — no new schema on either side.

use std::time::Duration;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pythonize::{depythonize, pythonize};

use commonmeta::Data;

fn to_py_err(e: commonmeta::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}

fn records_from_py(records: &Bound<'_, PyAny>) -> PyResult<Vec<Data>> {
    depythonize(records).map_err(|e| PyValueError::new_err(e.to_string()))
}

fn records_to_py<'py>(py: Python<'py>, list: &[Data]) -> PyResult<Bound<'py, PyAny>> {
    pythonize(py, list).map_err(|e| PyValueError::new_err(e.to_string()))
}

/// Fetch commonmeta records from a VRAIX daily dump.
///
/// `source` is `"crossref"` or `"datacite"`; `date` is `YYYY-MM-DD`. With
/// `input_path`, the local SQLite file at that path is read directly
/// (no network); otherwise `{source}-{date}.sqlite3.zst` is downloaded from
/// metadata.vraix.org and cached locally for `cache_ttl_days`.
/// `limit`/`offset` window the rows read; `limit=None` reads every row.
#[pyfunction]
#[pyo3(signature = (source, date, input_path=None, limit=None, offset=0, cache_ttl_days=30))]
fn fetch_vraix<'py>(
    py: Python<'py>,
    source: &str,
    date: &str,
    input_path: Option<&str>,
    limit: Option<usize>,
    offset: usize,
    cache_ttl_days: u64,
) -> PyResult<Bound<'py, PyAny>> {
    let cache_ttl = Duration::from_secs(cache_ttl_days * 24 * 60 * 60);
    let list = commonmeta::fetch_vraix_dump(source, date, input_path, limit, offset, cache_ttl)
        .map_err(to_py_err)?;
    records_to_py(py, &list)
}

/// Write a list of commonmeta record dicts as a single lossless Parquet file.
#[pyfunction]
fn write_parquet<'py>(
    py: Python<'py>,
    records: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyBytes>> {
    let list = records_from_py(records)?;
    let bytes = commonmeta::write_parquet(&list).map_err(to_py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

/// Read commonmeta record dicts back from Parquet bytes written by
/// `write_parquet`.
#[pyfunction]
fn read_parquet<'py>(py: Python<'py>, data: &[u8]) -> PyResult<Bound<'py, PyAny>> {
    let list = commonmeta::read_parquet(data).map_err(to_py_err)?;
    records_to_py(py, &list)
}

/// Render a list of commonmeta record dicts to `to` format, split into
/// batches of at most `batch_size` records, returned as
/// `(entry_name, bytes)` pairs ready to pack into a zip/tar archive.
#[pyfunction]
#[pyo3(signature = (records, to, base_name, batch_size=100_000))]
fn write_archive<'py>(
    py: Python<'py>,
    records: &Bound<'py, PyAny>,
    to: &str,
    base_name: &str,
    batch_size: usize,
) -> PyResult<Vec<(String, Bound<'py, PyBytes>)>> {
    let list = records_from_py(records)?;
    let entries = commonmeta::write_archive(&list, to, base_name, batch_size).map_err(to_py_err)?;
    Ok(entries
        .into_iter()
        .map(|(name, bytes)| (name, PyBytes::new(py, &bytes)))
        .collect())
}

/// Convert a single record from `from_` format to `to` format.
#[pyfunction]
fn convert<'py>(
    py: Python<'py>,
    from_: &str,
    to: &str,
    input: &str,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = commonmeta::convert(from_, to, input).map_err(to_py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pymodule]
fn commonmeta_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(fetch_vraix, m)?)?;
    m.add_function(wrap_pyfunction!(write_parquet, m)?)?;
    m.add_function(wrap_pyfunction!(read_parquet, m)?)?;
    m.add_function(wrap_pyfunction!(write_archive, m)?)?;
    m.add_function(wrap_pyfunction!(convert, m)?)?;
    Ok(())
}
