use crate::{InspectOptions, InspectReport, binary::Source, xml};
use std::path::Path;

pub(crate) fn inspect(path: &Path, report: &mut InspectReport, options: &InspectOptions) {
    let result = (|| -> xml::Result<()> {
        let mut source = Source::open(path)?;
        let bytes = source.range(0, source.len as usize)?;
        let text = std::str::from_utf8(&bytes)?.trim_start_matches('\u{feff}');
        report.append_text(text, options.max_chars);
        if report.format.extension == "md" {
            crate::markdown::append(report, text, options, "native_structure");
        }
        Ok(())
    })();
    if let Err(error) = result {
        report.error("text", error);
    }
}
