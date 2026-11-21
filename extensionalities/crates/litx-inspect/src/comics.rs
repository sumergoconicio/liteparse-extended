use crate::{Claim, InspectOptions, InspectReport, Page, archive::Archive, office, xml};

pub(crate) fn image_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "avif"
        )
    })
}

pub(crate) fn cbz(archive: &mut Archive, report: &mut InspectReport, options: &InspectOptions) {
    if report.format.extension != "cbz" {
        return;
    }
    for name in archive.names.clone().into_iter().filter(|name| {
        image_name(name)
            && !name
                .split('/')
                .any(|part| part == "__MACOSX" || part.starts_with("._"))
    }) {
        let result = (|| -> xml::Result<()> {
            let bytes = archive.read(&name)?.ok_or("missing comic image entry")?;
            let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
                .with_guessed_format()?
                .into_dimensions()?;
            if width == 0 || height == 0 || width > 16_384 || height > 16_384 {
                return Err("comic image dimension cap exceeded".into());
            }
            report.pages.push(Page {
                index: report.pages.len() + 1,
                source_part: name.clone(),
                width: Some(width),
                height: Some(height),
                dpi: None,
                version: None,
            });
            if report.pages.len() == 1 {
                report.claims.push(Claim {
                    key: "cover_entry".into(),
                    value: name.clone(),
                    source: "archive_entry",
                });
                if options.thumbnail.is_some() {
                    crate::thumbnail::write(
                        std::io::Cursor::new(bytes),
                        "embedded",
                        options,
                        report,
                    );
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            report.error("comic_page", format!("{name}: {error}"));
        }
    }
    if report.pages.is_empty() {
        report.error("comic", "CBZ contains no readable image pages");
    } else {
        office::identify(
            report,
            "cbz",
            "application/vnd.comicbook+zip",
            "ZIP comic book",
        );
        report.counts.pages = Some(report.pages.len());
        report.claims.push(Claim {
            key: "page_order".into(),
            value: "archive_entry_order".into(),
            source: "bytes",
        });
    }
}
