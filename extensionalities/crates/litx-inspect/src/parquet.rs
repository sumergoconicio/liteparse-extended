//! Parquet support boundary: identification only. Extraction is deferred by user
//! decision (2026-11-18, "defer extraction, make a note to return to it later").
//! Neither inspect nor extract parses a Parquet footer, schema, row, column, or
//! page; the deferred Arrow batch API candidate is recorded in the wave-3
//! remediation contract for a later approved task.
use crate::{InspectOptions, InspectReport};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct ParquetProfile {
    pub basis: &'static str,
    pub rows: u64,
    pub row_groups: usize,
    pub row_group_rows: Vec<u64>,
    pub leaf_columns: usize,
    pub columns: Vec<ParquetColumn>,
    pub created_by: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ParquetColumn {
    pub path: String,
    pub path_segments: Vec<String>,
    pub physical_type: &'static str,
    pub logical_type: Option<String>,
}

/// Static support boundary. The detector has already identified the format from
/// its bounded prefix; no attacker-controlled metadata is read here. The omitted
/// text is marked incomplete rather than reported as a successful empty
/// extraction.
pub(crate) fn inspect(_path: &Path, report: &mut InspectReport, _options: &InspectOptions) {
    report.text.truncated = true;
    report.error(
        "parquet",
        "Parquet extraction is deferred; identification only",
    );
}
