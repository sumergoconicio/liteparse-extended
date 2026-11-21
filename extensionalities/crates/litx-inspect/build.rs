fn main() {
    #[cfg(feature = "ffmpeg-link")]
    println!(
        "cargo:warning=ffmpeg-link enabled: linking a GPL-configured FFmpeg can impose GPL obligations on the combined binary. Default builds do not link FFmpeg."
    );
}
