use crate::{InspectOptions, InspectReport, Thumbnail};
use image::{ImageFormat, ImageReader, Limits};
use std::{
    fs::OpenOptions,
    io::{BufRead, Read, Seek, Write},
};

pub(crate) fn write<R: BufRead + Seek>(
    input: R,
    basis: &'static str,
    options: &InspectOptions,
    report: &mut InspectReport,
) {
    let Some(path) = &options.thumbnail else {
        return;
    };
    let result = (|| -> Result<Thumbnail, Box<dyn std::error::Error>> {
        if !(1..=4096).contains(&options.thumbnail_max_edge) {
            return Err("thumbnail edge must be between 1 and 4096".into());
        }
        let mut bytes = Vec::new();
        input
            .take(crate::archive::MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > crate::archive::MAX_ENTRY_BYTES {
            return Err("thumbnail encoded-byte read cap exceeded".into());
        }
        let mut reader = ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
        let mut limits = Limits::default();
        limits.max_image_width = Some(16_384);
        limits.max_image_height = Some(16_384);
        limits.max_alloc = Some(128 << 20);
        reader.limits(limits);
        let image = reader.decode()?;
        let fits = image.width().max(image.height()) <= options.thumbnail_max_edge;
        let image = if fits {
            image
        } else {
            image.thumbnail(options.thumbnail_max_edge, options.thumbnail_max_edge)
        };
        let encoded = if basis == "embedded" && fits {
            bytes
        } else {
            let mut output = std::io::Cursor::new(Vec::new());
            image.write_to(&mut output, ImageFormat::Png)?;
            output.into_inner()
        };
        // Never truncate an existing file, including a symlink or the input.
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&encoded)?;
        Ok(Thumbnail {
            written_to: path.clone(),
            width: image.width(),
            height: image.height(),
            basis,
        })
    })();
    match result {
        Ok(thumbnail) => report.thumbnail = Some(thumbnail),
        Err(error) => report.error("thumbnail", error),
    }
}
