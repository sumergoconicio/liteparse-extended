use crate::{InspectOptions, InspectReport, xml};
use serde::Serialize;
use serde_json::Value;
use std::{io::Read, path::Path};

#[derive(Debug, Serialize)]
pub struct DatasetPreview {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub row_limit: usize,
    pub has_more: bool,
    pub values_truncated: bool,
}

pub(crate) fn render(
    preview: &DatasetPreview,
    report: &mut InspectReport,
    options: &InspectOptions,
) {
    let mut markdown = String::from("## Dataset preview\n\n");
    let cell = |value: &str| {
        crate::markdown::escape(value)
            .replace('|', "\\|")
            .replace(['\n', '\r', '\t'], "<br>")
    };
    markdown.push_str(&format!(
        "| {} |\n",
        preview
            .headers
            .iter()
            .map(|s| cell(s))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    markdown.push_str(&format!(
        "|{}|\n",
        vec![" --- "; preview.headers.len()].join("|")
    ));
    for row in &preview.rows {
        let values = row
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string())
            })
            .collect::<Vec<_>>();
        markdown.push_str(&format!(
            "| {} |\n",
            values
                .iter()
                .map(|s| cell(s))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
        report.append_text(&format!("{}\n", values.join("\t")), options.max_chars);
    }
    markdown.push_str(
        "\n> Dataset preview: at most five data rows. The source dataset was not exported.\n\n",
    );
    crate::markdown::append(report, &markdown, options, "native_structure");
}

pub(crate) fn csv(path: &Path, report: &mut InspectReport, options: &InspectOptions) {
    let result = (|| -> xml::Result<()> {
        const MAX_BYTES: u64 = 1 << 20;
        let file = std::fs::File::open(path)?;
        let delimiter = if report.format.extension == "tsv" {
            b'\t'
        } else {
            b','
        };
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .from_reader(file.take(MAX_BYTES + 1));
        let headers = reader
            .headers()?
            .iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if headers.len() > 16_384 || reader.position().byte() > MAX_BYTES {
            return Err("CSV header byte/column cap exceeded".into());
        }
        let mut preview = DatasetPreview {
            headers,
            rows: vec![],
            row_limit: 5,
            has_more: false,
            values_truncated: false,
        };
        let mut record = csv::StringRecord::new();
        for index in 0..6 {
            if !reader.read_record(&mut record)? {
                break;
            }
            if reader.position().byte() > MAX_BYTES {
                return Err("CSV preview exceeds 1 MiB read cap".into());
            }
            if index == 5 {
                preview.has_more = true;
                break;
            }
            preview.rows.push(
                record
                    .iter()
                    .map(|v| {
                        preview.values_truncated |= v.chars().count() > 512;
                        Value::String(v.chars().take(512).collect())
                    })
                    .collect(),
            );
        }
        render(&preview, report, options);
        report.dataset = Some(preview);
        Ok(())
    })();
    if let Err(error) = result {
        report.error("csv", error);
    }
}
