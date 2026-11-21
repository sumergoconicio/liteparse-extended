use clap::{Args, Parser, Subcommand, ValueEnum};
use liteparse::LiteParseConfig;
use litx_inspect::{InspectOptions, Inspector, inspect};
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "glassbox",
    version,
    about = "What's in the box?! Local file inspection and extraction."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Inspect file identity, native content, structure, and embedded previews offline.
    Inspect(InspectArgs),
    /// Extract a whole document, with local OCR and first-class Markdown.
    Extract(InspectArgs),
    /// Print the upstream library's default configuration as JSON.
    Defaults,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    Json,
    Text,
    Markdown,
}

#[derive(Debug, Args)]
struct InspectArgs {
    #[arg(required_unless_present = "batch", conflicts_with = "batch")]
    path: Option<PathBuf>,
    /// Read one {"id":...,"path":"..."} JSON request per stdin line.
    #[arg(long, conflicts_with_all = ["pretty", "thumbnail" , "format"])]
    batch: bool,
    /// Save a Markdown file and JSON report per document, without overwriting.
    #[arg(long)]
    output_dir: Option<PathBuf>,
    /// Identification preview: five PDF pages, local OCR, and 500 word tokens by default.
    #[arg(long)]
    identify: bool,
    /// Whitespace-delimited word tokens, not model-specific subword tokens.
    #[arg(long)]
    max_tokens: Option<usize>,
    #[arg(long)]
    max_pages: Option<usize>,
    #[arg(long, conflicts_with_all = ["probe_ocr", "identify"])]
    no_ocr: bool,
    #[arg(long)]
    max_chars: Option<usize>,
    #[arg(long, default_value_t = 5)]
    max_units: usize,
    #[arg(long)]
    thumbnail: Option<PathBuf>,
    #[arg(long, default_value_t = 320, value_parser = clap::value_parser!(u32).range(1..=4096))]
    thumbnail_max_edge: u32,
    #[arg(long)]
    packet: bool,
    #[arg(long, value_enum, default_value_t = Format::Json)]
    format: Format,
    #[arg(long)]
    probe_ocr: bool,
    #[arg(long)]
    pretty: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let full_document = matches!(&cli.command, Commands::Extract(_));
    match cli.command {
        Commands::Inspect(args) | Commands::Extract(args) => {
            let options = InspectOptions {
                max_chars: args
                    .max_chars
                    .unwrap_or(if full_document { 64 << 20 } else { 16_000 }),
                max_pages: args
                    .max_pages
                    .or(if full_document { None } else { Some(5) }),
                max_units: args.max_units,
                thumbnail: args.thumbnail,
                thumbnail_max_edge: args.thumbnail_max_edge,
                packet: args.packet,
                probe_ocr: args.probe_ocr,
                identify: !args.no_ocr && (args.identify || full_document),
                max_tokens: args.max_tokens.or((!full_document).then_some(500)),
            };
            if args.batch {
                return run_batch(&options, args.output_dir.as_deref());
            }
            let mut report = inspect(args.path.as_ref().unwrap(), &options);
            if let Some(directory) = args.output_dir {
                persist(&mut report, &directory, &run_id(), 1);
            }
            match args.format {
                Format::Markdown => println!("{}", report.markdown.content),
                Format::Text => println!("{}", report.quick_look()),
                Format::Json if args.pretty => {
                    println!("{}", serde_json::to_string_pretty(&report)?)
                }
                Format::Json => println!("{}", serde_json::to_string(&report)?),
            }
        }
        Commands::Defaults => {
            println!(
                "{}",
                serde_json::to_string_pretty(&LiteParseConfig::default())?
            );
        }
    }
    Ok(())
}

fn run_batch(
    options: &InspectOptions,
    output_dir: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut inspector = Inspector::default();
    let run_id = run_id();
    let mut sequence = 0usize;
    let input = std::io::stdin();
    let mut input = input.lock();
    let mut output = std::io::stdout().lock();
    const MAX_LINE: usize = 65_536;
    loop {
        let mut line = Vec::new();
        if input
            .by_ref()
            .take((MAX_LINE + 1) as u64)
            .read_until(b'\n', &mut line)?
            == 0
        {
            break;
        }
        let response = if line.len() > MAX_LINE {
            if line.last() != Some(&b'\n') {
                input.skip_until(b'\n')?;
            }
            serde_json::json!({"id":null,"error":{
                "stage":"batch","message":"request exceeds 65536-byte line limit"}})
        } else {
            match serde_json::from_slice::<serde_json::Value>(&line) {
                Ok(request) => match request.get("path").and_then(|p| p.as_str()) {
                    Some(path) => {
                        sequence += 1;
                        let mut report = inspector.inspect(std::path::Path::new(path), options);
                        if let Some(directory) = output_dir {
                            persist(&mut report, directory, &run_id, sequence);
                        }
                        serde_json::json!({
                            "id": request.get("id"), "path": path,
                            "report": report,
                        })
                    }
                    None => serde_json::json!({"id":request.get("id"), "error":{
                    "stage":"batch", "message":"request requires a string path"}}),
                },
                Err(error) => serde_json::json!({"id":null, "error":{
                "stage":"batch", "message":error.to_string()}}),
            }
        };
        serde_json::to_writer(&mut output, &response)?;
        writeln!(output)?;
        output.flush()?;
    }
    Ok(())
}

fn run_id() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

fn persist(
    report: &mut litx_inspect::InspectReport,
    directory: &std::path::Path,
    run: &str,
    sequence: usize,
) {
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(directory)?;
        let directory = directory.canonicalize()?;
        let markdown = directory.join(format!("{run}-{sequence:06}.md"));
        let json = directory.join(format!("{run}-{sequence:06}.json"));
        write_new(&markdown, report.markdown.content.as_bytes())?;
        report.artifacts.push(litx_inspect::Artifact {
            kind: "markdown",
            path: markdown,
        });
        report.artifacts.push(litx_inspect::Artifact {
            kind: "json",
            path: json.clone(),
        });
        let bytes = serde_json::to_vec(report).map_err(std::io::Error::other)?;
        if let Err(error) = write_new(&json, &bytes) {
            report.artifacts.pop();
            return Err(error);
        }
        Ok(())
    })();
    if let Err(error) = result {
        report.errors.push(litx_inspect::StageError {
            stage: "output",
            message: error.to_string(),
        });
    }
}

fn write_new(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_command_is_available() {
        for name in ["glassbox", "litx"] {
            let cli = Cli::try_parse_from([name, "defaults"]).unwrap();
            assert!(matches!(cli.command, Commands::Defaults));
        }
    }

    #[test]
    fn upstream_defaults_serialize_as_json() {
        let config = serde_json::to_value(LiteParseConfig::default()).unwrap();
        assert!(config.is_object());
        assert_eq!(config["ocr_language"], "eng");
    }
}
