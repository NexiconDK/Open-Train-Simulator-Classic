//! `opentsc`: command line tools for looking inside a RailWorks / Train
//! Simulator Classic installation. Everything here only reads.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use opentsc_formats::ap::ApArchive;
use opentsc_formats::pak::Pak;
use opentsc_formats::xml::WriteOptions;
use opentsc_formats::{parse_document, serz, xml, Install};

const USAGE: &str = "\
opentsc - inspect a RailWorks / Train Simulator Classic installation (read-only)

USAGE:
  opentsc routes <RailWorks folder>
      List the installed routes.

  opentsc bin2xml <file> [output.xml] [--elide-blobs]
      Decode a SERZ binary (.bin, .proxybin, .GeoPcDx, .TgPcDx, ...) to XML.
      Writes to standard output unless an output file is given.

  opentsc ap list <archive.ap>
  opentsc ap extract <archive.ap> <entry> <output file>
      List or extract the contents of an .ap archive.

  opentsc pak list <Blueprints.pak>
  opentsc pak extract <Blueprints.pak> <entry> <output file>
      List or extract the contents of a blueprint cache.

  opentsc blueprint <RailWorks folder> <Provider> <Product> <path> [--elide-blobs]
      Resolve a blueprint the way content refers to it and print it as XML.
      Example: opentsc blueprint D:\\RailWorks Kuju RailSimulator Scenery\\Tree.xml

  opentsc scan <folder> [--max-errors N]
      Parse every SERZ file under a folder, including files inside .ap and
      .pak archives, and report what could not be read.
";

type CliResult = Result<ExitCode, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let elide_blobs = take_flag(&mut args, "--elide-blobs");
    let max_errors = match take_option(&mut args, "--max-errors") {
        Ok(value) => value.unwrap_or(20),
        Err(message) => return fail(&message),
    };
    let options = WriteOptions { elide_blobs };
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    let result = match args.as_slice() {
        ["routes", root] => routes(root),
        ["bin2xml", input] => bin2xml(input, None, options),
        ["bin2xml", input, output] => bin2xml(input, Some(output), options),
        ["ap", "list", archive] => ap_list(archive),
        ["ap", "extract", archive, entry, output] => ap_extract(archive, entry, output),
        ["pak", "list", pak] => pak_list(pak),
        ["pak", "extract", pak, entry, output] => pak_extract(pak, entry, output),
        ["blueprint", root, provider, product, path] => {
            blueprint(root, provider, product, path, options)
        }
        ["scan", folder] => scan(folder, max_errors),
        [] | ["help"] | ["--help"] | ["-h"] => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            eprint!("{USAGE}");
            Ok(ExitCode::from(2))
        }
    };
    match result {
        Ok(code) => code,
        Err(error) => fail(&error.to_string()),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::FAILURE
}

fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != flag);
    args.len() != before
}

fn take_option(args: &mut Vec<String>, name: &str) -> Result<Option<usize>, String> {
    let Some(at) = args.iter().position(|a| a == name) else {
        return Ok(None);
    };
    if at + 1 >= args.len() {
        return Err(format!("{name} needs a number"));
    }
    let value = args[at + 1]
        .parse()
        .map_err(|_| format!("{name} needs a number"))?;
    args.drain(at..=at + 1);
    Ok(Some(value))
}

/// Write to standard output, treating a closed pipe (`| head`) as success.
fn print_all(text: &str) -> CliResult {
    match std::io::stdout().lock().write_all(text.as_bytes()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e.into()),
        _ => Ok(ExitCode::SUCCESS),
    }
}

fn routes(root: &str) -> CliResult {
    let install = Install::open(root)?;
    let routes = install.routes()?;
    let mut out = String::new();
    for route in &routes {
        out.push_str(&format!(
            "{}  {}{}\n",
            route.id,
            route.name.as_deref().unwrap_or("(name unreadable)"),
            if route.packed { "  [packed]" } else { "" },
        ));
    }
    out.push_str(&format!("{} routes\n", routes.len()));
    print_all(&out)
}

fn bin2xml(input: &str, output: Option<&str>, options: WriteOptions) -> CliResult {
    let doc = serz::parse(&fs::read(input)?)?;
    let text = xml::to_string_with(&doc, options);
    match output {
        Some(path) => {
            fs::write(path, text)?;
            Ok(ExitCode::SUCCESS)
        }
        None => print_all(&text),
    }
}

fn ap_list(archive: &str) -> CliResult {
    let ap = ApArchive::open(archive)?;
    let mut out = ap.names().join("\n");
    out.push_str(&format!("\n{} files\n", ap.len()));
    print_all(&out)
}

fn ap_extract(archive: &str, entry: &str, output: &str) -> CliResult {
    fs::write(output, ApArchive::open(archive)?.read(entry)?)?;
    Ok(ExitCode::SUCCESS)
}

fn pak_list(path: &str) -> CliResult {
    let pak = Pak::parse(fs::read(path)?)?;
    let mut out = String::new();
    for entry in pak.entries() {
        out.push_str(&format!("{:>9}  {}\n", entry.size, entry.name));
    }
    out.push_str(&format!("{} blueprints\n", pak.entries().len()));
    print_all(&out)
}

fn pak_extract(path: &str, entry: &str, output: &str) -> CliResult {
    let pak = Pak::parse(fs::read(path)?)?;
    let found = pak
        .find(entry)
        .ok_or_else(|| format!("no entry '{entry}' in {path}"))?;
    fs::write(output, pak.data(found))?;
    Ok(ExitCode::SUCCESS)
}

fn blueprint(
    root: &str,
    provider: &str,
    product: &str,
    path: &str,
    options: WriteOptions,
) -> CliResult {
    let doc = Install::open(root)?.load_blueprint(provider, product, path)?;
    print_all(&xml::to_string_with(&doc, options))
}

/// Running totals for `scan`.
#[derive(Default)]
struct Scan {
    /// Lower-cased extension -> (parsed, failed).
    by_extension: BTreeMap<String, (u64, u64)>,
    bytes: u64,
    errors: Vec<String>,
    max_errors: usize,
    unreadable: u64,
}

impl Scan {
    /// Parse one SERZ or XML document and record the outcome.
    fn check(&mut self, label: &str, data: &[u8]) {
        let extension = Path::new(label)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let counts = self.by_extension.entry(extension).or_default();
        self.bytes += data.len() as u64;
        match parse_document(data) {
            Ok(_) => counts.0 += 1,
            Err(error) => {
                counts.1 += 1;
                if self.errors.len() < self.max_errors {
                    self.errors.push(format!("{label}: {error}"));
                }
            }
        }
    }

    fn note_unreadable(&mut self, label: &str, error: &dyn std::fmt::Display) {
        self.unreadable += 1;
        if self.errors.len() < self.max_errors {
            self.errors.push(format!("{label}: {error}"));
        }
    }
}

fn scan(folder: &str, max_errors: usize) -> CliResult {
    let started = Instant::now();
    let mut scan = Scan {
        max_errors,
        ..Default::default()
    };
    let mut pending: Vec<PathBuf> = vec![PathBuf::from(folder)];
    let mut visited: u64 = 0;

    while let Some(dir) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                scan.note_unreadable(&dir.display().to_string(), &error);
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // file_type() does not follow symlinks, so links are never walked.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                visited += 1;
                if visited % 20_000 == 0 {
                    eprintln!("  ... {visited} files looked at");
                }
                scan_file(&mut scan, &path);
            }
        }
    }

    let (mut ok, mut failed) = (0u64, 0u64);
    println!("{:<12} {:>10} {:>8}", "extension", "parsed", "failed");
    for (extension, (parsed, bad)) in &scan.by_extension {
        let shown = if extension.is_empty() {
            "(none)"
        } else {
            extension
        };
        println!("{shown:<12} {parsed:>10} {bad:>8}");
        ok += parsed;
        failed += bad;
    }
    println!(
        "\n{ok} documents parsed, {failed} failed, {} unreadable files or archives; \
         {:.1} MiB in {:.1} s",
        scan.unreadable,
        scan.bytes as f64 / (1024.0 * 1024.0),
        started.elapsed().as_secs_f64()
    );
    if !scan.errors.is_empty() {
        println!("\nFirst {} problems:", scan.errors.len());
        for error in &scan.errors {
            println!("  {error}");
        }
    }
    Ok(if failed == 0 && scan.unreadable == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn scan_file(scan: &mut Scan, path: &Path) {
    let label = path.display().to_string();
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "ap" => match ApArchive::open(path) {
            Ok(mut ap) => {
                for name in ap.names().to_vec() {
                    match ap.read_if_magic(&name, serz::MAGIC) {
                        Ok(Some(data)) => scan.check(&format!("{label}!{name}"), &data),
                        Ok(None) => {}
                        Err(error) => scan.note_unreadable(&format!("{label}!{name}"), &error),
                    }
                }
            }
            Err(error) => scan.note_unreadable(&label, &error),
        },
        "pak" => match fs::read(path).map_err(Into::into).and_then(Pak::parse) {
            Ok(pak) => {
                for entry in pak.entries() {
                    scan.check(&format!("{label}!{}", entry.name), pak.data(entry));
                }
            }
            Err(error) => scan.note_unreadable(&label, &error),
        },
        _ => {
            // Decide by content, not by name: plenty of extensions hold SERZ.
            let mut magic = [0u8; 4];
            let is_serz = fs::File::open(path)
                .and_then(|mut file| file.read_exact(&mut magic))
                .is_ok_and(|()| &magic == serz::MAGIC);
            if is_serz {
                match fs::read(path) {
                    Ok(data) => scan.check(&label, &data),
                    Err(error) => scan.note_unreadable(&label, &error),
                }
            }
        }
    }
}
