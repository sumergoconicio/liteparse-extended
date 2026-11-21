//! Structured-data profiling for the `litx-inspect` crate.
//! File: `profile.rs`.
//! Responsibility: enforce grid and workbook work budgets while computing
//! bounded column telemetry and prose scores, not language detection.
use crate::{Column, Sheet, xml};
use std::collections::{BTreeMap, HashSet};
use unicode_general_category::{GeneralCategory, get_general_category};

pub(crate) const MAX_COLUMNS: usize = 16_384;
pub(crate) const MAX_HASHES: usize = 524_288;
const STOPWORDS: &str = "a about above after again against all am an and any are as at be because been before being below between both but by can could did do does doing down during each few for from further had has have having he her here hers herself him himself his how i if in into is it its itself just me more most my myself no nor not now of off on once only or other our ours ourselves out over own same should so some such than that the their theirs them themselves then there these they this those through to too under until up very was we were what when where which while who whom why will with would you your yours yourself yourselves";

pub(crate) fn prose_density(value: &str) -> f64 {
    let text = value.trim();
    if text.len() < 15 {
        return 0.0;
    }
    let (mut words, mut stops, mut symbols, mut sentences, mut sentence_words) = (0, 0, 0, 0, 0);
    let mut valid = true;
    for word in text.split_whitespace() {
        words += 1;
        symbols += word
            .bytes()
            .filter(|b| b"{}[]();=<>\\|~^`".contains(b))
            .count();
        let cleaned = word.trim_matches(|c| ".,!?:;\"'()[]{}-_".contains(c));
        if STOPWORDS
            .split(' ')
            .any(|s| s.eq_ignore_ascii_case(cleaned))
        {
            stops += 1;
        } else if stops == 0 && valid {
            let letters = word.trim_end_matches(['.', '!', '?']);
            valid = !letters.is_empty()
                && letters.chars().all(|c| {
                    matches!(
                        get_general_category(c),
                        GeneralCategory::UppercaseLetter
                            | GeneralCategory::LowercaseLetter
                            | GeneralCategory::TitlecaseLetter
                            | GeneralCategory::ModifierLetter
                            | GeneralCategory::OtherLetter
                    )
                })
                && (sentence_words != 0
                    || letters.chars().next().is_some_and(|c| {
                        get_general_category(c) == GeneralCategory::UppercaseLetter
                    }));
            if valid {
                sentence_words += 1;
                if letters.len() != word.len() {
                    if sentence_words < 3 {
                        valid = false;
                    } else {
                        sentences += 1;
                        sentence_words = 0;
                    }
                }
            }
        }
    }
    if words < 3 {
        return 0.0;
    }
    let ratio = if stops > 0 {
        stops as f64 / words as f64
    } else if valid && sentence_words == 0 && sentences > 0 {
        sentences as f64 / words as f64
    } else {
        0.0
    };
    ((ratio * 2.2)
        * (1.0 - (symbols as f64 / text.len() as f64 * 4.0).min(1.0))
        * (text.len() as f64 / 120.0).min(1.0))
    .clamp(0.0, 1.0)
}

#[cfg(test)]
mod reference_tests {
    #[test]
    fn prose_scores_match_jsoninspect_reference() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/prose-reference.json")).unwrap();
        for sample in reference["samples"].as_array().unwrap() {
            let text = sample["text"].as_str().unwrap();
            let expected = sample["score"].as_f64().unwrap();
            let actual = super::prose_density(text);
            assert!(
                (actual - expected).abs() < 1e-12,
                "{text:?}: {actual} != {expected}"
            );
        }
    }
}

pub(crate) fn preview(value: &str) -> String {
    if value.len() <= 60 {
        return value.into();
    }
    let mut end = 60;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &value[..end])
}

/// Maximum logical cell visits charged across one spreadsheet workbook.
pub(crate) const WORKBOOK_CELL_CAP: usize = 262_144;
/// Maximum logical value bytes scanned across one spreadsheet workbook.
pub(crate) const WORKBOOK_VALUE_BYTE_CAP: usize = 64 << 20;
/// Diagnostic emitted when cumulative workbook value-byte work is exhausted.
pub(crate) const WORK_VALUE_BUDGET_EXCEEDED: &str =
    "spreadsheet workbook value-byte budget exhausted";

/// Cumulative logical cell and value-byte work remaining for one workbook.
/// Failed and empty sheets share this budget with successfully retained sheets.
pub(crate) struct WorkBudget {
    cells_remaining: usize,
    value_bytes_remaining: usize,
    exhausted: bool,
}

impl WorkBudget {
    /// Creates an independent work budget with explicit limits.
    /// Accepts logical cell-visit and value-byte caps; returns the initialized
    /// budget; has no failure states.
    pub(crate) fn new(cell_cap: usize, value_byte_cap: usize) -> Self {
        Self {
            cells_remaining: cell_cap,
            value_bytes_remaining: value_byte_cap,
            exhausted: false,
        }
    }

    /// Charges logical cell visits before cell expansion or profiling.
    /// Accepts the number of repeated logical cells; returns unit while the
    /// visit count fits; returns an exhaustion error and stops the workbook
    /// when the count exceeds the remaining budget.
    pub(crate) fn charge_cells(&mut self, count: usize) -> xml::Result<()> {
        if count > self.cells_remaining {
            self.cells_remaining = 0;
            self.exhausted = true;
            return Err("spreadsheet workbook cell budget exhausted".into());
        }
        self.cells_remaining -= count;
        Ok(())
    }

    /// Charges logical value bytes before full-value scans and text append.
    /// Accepts the expanded byte count including repetitions; returns unit
    /// while the count fits; returns an exhaustion error and stops the
    /// workbook when the count exceeds the remaining budget.
    pub(crate) fn charge_value_bytes(&mut self, count: usize) -> xml::Result<()> {
        if count > self.value_bytes_remaining {
            self.value_bytes_remaining = 0;
            self.exhausted = true;
            return Err(WORK_VALUE_BUDGET_EXCEEDED.into());
        }
        self.value_bytes_remaining -= count;
        Ok(())
    }

    /// Returns the remaining value-byte budget for bounded ODF expansion.
    /// Accepts no inputs; returns the logical byte count still available;
    /// has no failure states.
    pub(crate) fn value_bytes_remaining(&self) -> usize {
        self.value_bytes_remaining
    }

    /// Marks value-byte work exhausted after a bounded ODF expansion rejects
    /// the next source text at the remaining cumulative limit.
    /// Accepts no inputs; returns unit; has no failure states.
    pub(crate) fn exhaust_value_bytes(&mut self) {
        self.value_bytes_remaining = 0;
        self.exhausted = true;
    }

    /// Reports whether either work limit rejected an attempted charge.
    /// Accepts no inputs; returns true only after budget exhaustion; has no
    /// failure states.
    pub(crate) fn is_exhausted(&self) -> bool {
        self.exhausted
    }
}

impl Default for WorkBudget {
    /// Creates the production workbook budget with approved cumulative caps.
    /// Accepts no inputs; returns a budget for 262144 cell visits and 64 MiB
    /// of logical value bytes; has no failure states.
    fn default() -> Self {
        Self::new(WORKBOOK_CELL_CAP, WORKBOOK_VALUE_BYTE_CAP)
    }
}

#[derive(Default)]
pub(crate) struct Budget {
    pub columns: usize,
    pub hashes: usize,
    pub capped: bool,
}

struct ColumnBuilder {
    column: Column,
    hashes: HashSet<u64>,
    score_sum: f64,
}

pub(crate) struct Grid {
    sheet: Sheet,
    columns: BTreeMap<usize, ColumnBuilder>,
    min_row: usize,
    min_col: usize,
}

impl Grid {
    pub fn new(name: String, index: usize) -> Self {
        Self {
            sheet: Sheet {
                preview_rows: vec![],
                preview_truncated: false,
                headers_preview: vec![],
                name,
                index,
                used_range: None,
                rows: 0,
                cols: 0,
                cell_count: 0,
                non_empty_cells: 0,
                formula_count: 0,
                merged_ranges: vec![],
                is_longest: false,
                columns: vec![],
            },
            columns: BTreeMap::new(),
            min_row: usize::MAX,
            min_col: usize::MAX,
        }
    }

    pub fn cell(
        &mut self,
        position: (usize, usize),
        value: &str,
        kind: &str,
        formula: bool,
        repeats: usize,
        budget: &mut Budget,
    ) -> xml::Result<()> {
        let (row, col) = position;
        let end_row = row
            .checked_add(repeats.saturating_sub(1))
            .ok_or("row overflow")?;
        if row == 0 || col == 0 || end_row > 1_048_576 || col > MAX_COLUMNS || repeats == 0 {
            return Err("spreadsheet row/column/repetition cap exceeded".into());
        }
        self.sheet.rows = self.sheet.rows.max(end_row);
        self.sheet.cols = self.sheet.cols.max(col);
        self.min_row = self.min_row.min(row);
        self.min_col = self.min_col.min(col);
        if formula {
            accumulate(&mut self.sheet.formula_count, 1, repeats);
        }
        if value.is_empty() {
            return Ok(());
        }
        accumulate(&mut self.sheet.non_empty_cells, 1, repeats);
        if let std::collections::btree_map::Entry::Vacant(entry) = self.columns.entry(col) {
            if budget.columns >= MAX_COLUMNS {
                budget.capped = true;
                return Ok(());
            }
            budget.columns += 1;
            entry.insert(ColumnBuilder {
                column: Column {
                    index: col,
                    header: None,
                    inferred_type: "empty".into(),
                    non_empty_count: 0,
                    distinct_count: 0,
                    total_bytes: 0,
                    max_bytes: 0,
                    mean_bytes: 0.0,
                    avg_prose_score: 0.0,
                    is_prose_candidate: false,
                    sample_values: vec![],
                },
                hashes: HashSet::new(),
                score_sum: 0.0,
            });
        }
        let builder = self.columns.get_mut(&col).expect("column inserted");
        let c = &mut builder.column;
        let mut count = repeats;
        // A string in physical row 1 is an explicit header candidate, not data.
        if row == 1 && kind == "string" && !formula {
            c.header = Some(preview(value));
            count -= 1;
        }
        if count == 0 {
            return Ok(());
        }
        if c.inferred_type == "empty" {
            c.inferred_type = kind.into();
        } else if c.inferred_type != kind {
            c.inferred_type = "mixed".into();
        }
        accumulate(&mut c.non_empty_count, 1, count);
        accumulate(&mut c.total_bytes, value.len(), count);
        c.max_bytes = c.max_bytes.max(value.len());
        let hash = value.bytes().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
        if !builder.hashes.contains(&hash) {
            if budget.hashes < MAX_HASHES {
                builder.hashes.insert(hash);
                budget.hashes += 1;
                c.distinct_count += 1;
            } else {
                budget.capped = true;
            }
        }
        if kind == "string" {
            let score = prose_density(value);
            builder.score_sum += score * count as f64;
            c.is_prose_candidate |= value.len() >= 35 && score >= 0.20;
            for _ in 0..count.min(3usize.saturating_sub(c.sample_values.len())) {
                c.sample_values.push(preview(value));
            }
        }
        Ok(())
    }

    pub fn merge(&mut self, range: String) -> xml::Result<()> {
        if self.sheet.merged_ranges.len() >= 4096 {
            return Err("merged range cap exceeded".into());
        }
        self.sheet.merged_ranges.push(range);
        Ok(())
    }

    pub fn finish(mut self) -> Sheet {
        self.sheet.cell_count = grid_cell_count(self.sheet.rows, self.sheet.cols);
        if self.sheet.rows > 0 {
            self.sheet.used_range = Some(format!(
                "{}{}:{}{}",
                column_name(self.min_col),
                self.min_row,
                column_name(self.sheet.cols),
                self.sheet.rows
            ));
        }
        self.sheet.columns = self
            .columns
            .into_values()
            .map(|mut b| {
                let n = b.column.non_empty_count;
                if n > 0 {
                    b.column.mean_bytes = b.column.total_bytes as f64 / n as f64;
                    b.column.avg_prose_score = b.score_sum / n as f64;
                }
                b.column
            })
            .collect();
        self.sheet.headers_preview = (1..=self.sheet.cols.min(5))
            .map(|index| {
                self.sheet
                    .columns
                    .iter()
                    .find(|c| c.index == index)
                    .and_then(|c| c.header.clone())
            })
            .collect();
        self.sheet
    }
}

/// Width-parameterized saturating arithmetic for telemetry counters.
/// Production instantiates `usize`; tests instantiate `u32` as a
/// width-reduced seam proving 32-bit targets (wasm32) saturate instead of
/// overflowing. Native 64-bit output is exact for every in-range value.
pub(crate) trait Saturating: Copy {
    fn sat_add(self, other: Self) -> Self;
    fn sat_mul(self, other: Self) -> Self;
}

impl Saturating for usize {
    fn sat_add(self, other: Self) -> Self {
        self.saturating_add(other)
    }
    fn sat_mul(self, other: Self) -> Self {
        self.saturating_mul(other)
    }
}

#[cfg(test)]
impl Saturating for u32 {
    fn sat_add(self, other: Self) -> Self {
        self.saturating_add(other)
    }
    fn sat_mul(self, other: Self) -> Self {
        self.saturating_mul(other)
    }
}

/// `total += value * count`, saturating at the type's maximum instead of
/// overflowing on narrow targets; exact for every in-range value.
pub(crate) fn accumulate<T: Saturating>(total: &mut T, value: T, count: T) {
    *total = total.sat_add(value.sat_mul(count));
}

/// Total grid cells, saturating rather than overflowing on narrow `usize` targets.
fn grid_cell_count(rows: usize, cols: usize) -> usize {
    rows.saturating_mul(cols)
}

#[cfg(test)]
mod bounds_tests {

    #[test]
    fn accumulate_saturates_at_reduced_width_instead_of_wrapping() {
        // Width-reduced seam for 32-bit targets (wasm32): the production
        // helper instantiated at u32 saturates where the previous unchecked
        // `+= value * count` arithmetic wrapped or panicked.
        let mut total = 0u32;
        super::accumulate(&mut total, 1u32 << 20, 4_096);
        assert_eq!(total, u32::MAX, "2^20 * 2^12 must saturate, not wrap to 0");
        let mut near = u32::MAX - 10;
        super::accumulate(&mut near, 1, 100);
        assert_eq!(near, u32::MAX, "addition past the cap must saturate");
    }

    #[test]
    fn accumulate_is_exact_on_wide_targets() {
        // Native 64-bit output is preserved: no saturation below usize::MAX.
        let mut total = 0usize;
        super::accumulate(&mut total, 1usize << 20, 1usize << 20);
        assert_eq!(total, 1usize << 40);
        super::accumulate(&mut total, 7, 3);
        assert_eq!(total, (1usize << 40) + 21);
    }

    #[test]
    fn grid_cell_accumulates_repeated_cell_telemetry_exactly() {
        // Regression guard for Grid::cell counters on wide targets.
        let mut grid = super::Grid::new("S".into(), 1);
        let mut budget = super::Budget::default();
        grid.cell((2, 1), "abcd", "string", true, 1_048_575, &mut budget)
            .unwrap();
        let sheet = grid.finish();
        assert_eq!(sheet.formula_count, 1_048_575);
        assert_eq!(sheet.non_empty_cells, 1_048_575);
        let column = &sheet.columns[0];
        assert_eq!(column.non_empty_count, 1_048_575);
        assert_eq!(column.total_bytes, 4 * 1_048_575);
    }
    #[test]
    fn grid_cell_count_saturates_instead_of_overflowing() {
        assert_eq!(super::grid_cell_count(1_048_576, 16_384), 17_179_869_184);
        assert_eq!(super::grid_cell_count(usize::MAX - 1, 4), usize::MAX);
        assert_eq!(super::grid_cell_count(0, 16_384), 0);
    }

    #[test]
    /// Verifies exact cell and value-byte boundaries without large fixtures.
    /// Accepts no inputs; returns unit; panics if exact charges fail, zero
    /// charges are rejected, or the first excess charge does not latch exhaustion.
    fn workbook_work_budget_accepts_exact_boundaries_and_rejects_excess() {
        let mut cells = super::WorkBudget::new(4, 8);
        cells.charge_cells(4).unwrap();
        cells.charge_cells(0).unwrap();
        assert!(!cells.is_exhausted());
        let error = cells.charge_cells(1).unwrap_err();
        assert!(error.to_string().contains("cell budget"), "{error}");
        assert!(cells.is_exhausted());

        let mut bytes = super::WorkBudget::new(4, 8);
        bytes.charge_value_bytes(8).unwrap();
        assert_eq!(bytes.value_bytes_remaining(), 0);
        bytes.charge_value_bytes(0).unwrap();
        assert!(!bytes.is_exhausted());
        let error = bytes.charge_value_bytes(1).unwrap_err();
        assert!(error.to_string().contains("value-byte budget"), "{error}");
        assert!(bytes.is_exhausted());
    }
}

pub(crate) fn column_name(mut column: usize) -> String {
    let mut bytes = vec![];
    while column > 0 {
        column -= 1;
        bytes.push(b'A' + (column % 26) as u8);
        column /= 26;
    }
    bytes.reverse();
    String::from_utf8(bytes).expect("ASCII column")
}

pub(crate) fn position(value: &str) -> xml::Result<(usize, usize)> {
    let split = value
        .find(|c: char| c.is_ascii_digit())
        .ok_or("invalid cell coordinate")?;
    let (letters, row) = value.split_at(split);
    if letters.is_empty() || !letters.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err("invalid cell column".into());
    }
    let mut col = 0usize;
    for b in letters.bytes() {
        col = col
            .checked_mul(26)
            .and_then(|n| n.checked_add((b - b'A' + 1) as usize))
            .ok_or("column overflow")?;
    }
    let row: usize = row.parse()?;
    if row == 0 || row > 1_048_576 || col > MAX_COLUMNS {
        return Err("cell coordinate out of range".into());
    }
    Ok((row, col))
}
