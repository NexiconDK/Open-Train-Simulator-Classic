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

  opentsc map <RailWorks folder> <route> <output.svg> [--no-terrain]
      Draw a route's track from above. <route> is the route's folder name or
      part of its display name. Also prints how well the curves fit together.

  opentsc markers <RailWorks folder> <route> [text]
      List the named places of a route (platforms, sidings, destinations),
      optionally only those whose name contains the text.

  opentsc path <RailWorks folder> <route> <from> <to> [output.svg]
      Find the shortest way a train can take between two places and print
      its length. A place is a marker name (or the start of one) or
      `ribbon:<start of id>@<metres|start|end>`. An output file ending in
      .svg gets the route's map with the path in red; one ending in .tsv
      gets the list of ribbons travelled.

  opentsc track-points <RailWorks folder> <route> <output.tsv>
      Write a point every 5 m along every track centre line, as tab-separated
      text: ribbon id, distance along the ribbon, x, z, heading in degrees,
      rail height.

  opentsc scan <folder> [--max-errors N]
      Parse every SERZ file under a folder, including files inside .ap and
      .pak archives, and report what could not be read.
";

type CliResult = Result<ExitCode, Box<dyn std::error::Error>>;

/// `println!` into a string, for output that goes through [`print_all`].
macro_rules! outln {
    ($out:expr, $($arg:tt)*) => {{
        $out.push_str(&format!($($arg)*));
        $out.push('\n');
    }};
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let elide_blobs = take_flag(&mut args, "--elide-blobs");
    let no_terrain = take_flag(&mut args, "--no-terrain");
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
        ["map", root, route, output] => map(root, route, output, !no_terrain),
        ["track-points", root, route, output] => track_points(root, route, output),
        ["markers", root, route] => markers(root, route, ""),
        ["markers", root, route, text] => markers(root, route, text),
        ["path", root, route, from, to] => path(root, route, from, to, None),
        ["path", root, route, from, to, output] => path(root, route, from, to, Some(output)),
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

/// Open the route whose folder name is `wanted` or whose display name
/// contains it.
fn find_route(
    root: &str,
    wanted: &str,
) -> Result<opentsc_route::Route, Box<dyn std::error::Error>> {
    let routes = Install::open(root)?.routes()?;
    let needle = wanted.to_lowercase();
    let matches: Vec<_> = routes
        .iter()
        .filter(|r| {
            r.id.eq_ignore_ascii_case(wanted)
                || r.name
                    .as_deref()
                    .is_some_and(|n| n.to_lowercase().contains(&needle))
        })
        .collect();
    let info = match matches.as_slice() {
        [one] => *one,
        [] => return Err(format!("no route matches '{wanted}'; see `opentsc routes`").into()),
        many => {
            let names: Vec<String> = many
                .iter()
                .map(|r| format!("{} ({})", r.name.as_deref().unwrap_or("?"), r.id))
                .collect();
            return Err(format!("'{wanted}' matches several routes: {}", names.join(", ")).into());
        }
    };

    Ok(opentsc_route::Route::open(&info.path)?)
}

fn track_points(root: &str, wanted: &str, output: &str) -> CliResult {
    use opentsc_route::track::assemble;
    use std::collections::BTreeMap;

    let route = find_route(root, wanted)?;
    let mut by_ribbon: BTreeMap<String, Vec<opentsc_route::geom::Curve>> = BTreeMap::new();
    for coord in route.track_tile_coords()? {
        for ribbon in route.track_tile(coord)?.ribbons {
            by_ribbon
                .entry(ribbon.ribbon_id)
                .or_default()
                .extend(ribbon.curves);
        }
    }
    let heights: std::collections::HashMap<String, opentsc_route::track::Ribbon> = route
        .track_network()?
        .ribbons
        .into_iter()
        .map(|ribbon| (ribbon.id.clone(), ribbon))
        .collect();
    let mut out = String::from("ribbon\tdistance\tx\tz\theading\theight\n");
    let mut points = 0usize;
    for (id, curves) in by_ribbon {
        let (curves, _) = assemble(curves);
        let mut before = 0.0;
        for curve in &curves {
            let steps = (curve.length / 5.0).ceil().max(1.0) as usize;
            for i in 0..=steps {
                let s = curve.length * i as f64 / steps as f64;
                let pose = curve.pose_at(s);
                let height = heights
                    .get(&id)
                    .and_then(|ribbon| ribbon.height_at(before + s))
                    .map_or(String::new(), |h| format!("{h:.3}"));
                out.push_str(&format!(
                    "{id}\t{:.2}\t{:.3}\t{:.3}\t{:.3}\t{height}\n",
                    before + s,
                    pose.position[0],
                    pose.position[1],
                    pose.heading.to_degrees()
                ));
                points += 1;
            }
            before += curve.length;
        }
    }
    fs::write(output, out)?;
    println!("wrote {points} points to {output}");
    Ok(ExitCode::SUCCESS)
}

fn markers(root: &str, wanted: &str, text: &str) -> CliResult {
    let route = find_route(root, wanted)?;
    let needle = text.to_lowercase();
    let mut found: Vec<_> = route
        .track_network()?
        .markers
        .into_iter()
        .filter(|m| m.name.to_lowercase().contains(&needle))
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    // Columns: name, kind, length, ribbon, distance of the middle along it.
    let mut out = String::new();
    for marker in &found {
        out.push_str(&format!(
            "{}\t{}\t{:.0} m\t{}\t{:.1}\n",
            marker.name,
            marker.kind,
            (marker.end - marker.start).abs(),
            marker.ribbon_id,
            (marker.start + marker.end) / 2.0
        ));
    }
    out.push_str(&format!("{} markers\n", found.len()));
    print_all(&out)
}

/// Turn a place given on the command line into a position on the network.
fn find_place(
    network: &opentsc_route::track::TrackNetwork,
    spec: &str,
) -> Result<(opentsc_route::graph::Place, String), Box<dyn std::error::Error>> {
    use opentsc_route::graph::Place;
    if let Some(rest) = spec.strip_prefix("ribbon:") {
        let (prefix, at) = rest
            .split_once('@')
            .ok_or("write ribbon:<id>@<metres|start|end>")?;
        let matches: Vec<_> = network
            .ribbons
            .iter()
            .filter(|r| r.id.starts_with(prefix))
            .collect();
        let [ribbon] = matches.as_slice() else {
            return Err(format!(
                "{} ribbons have an id starting with '{prefix}'",
                matches.len()
            )
            .into());
        };
        let distance = match at {
            "start" => 0.0,
            "end" => ribbon.length,
            metres => metres
                .parse()
                .map_err(|_| format!("'{metres}' is not a distance"))?,
        };
        let label = format!(
            "ribbon {} at {distance:.1} m",
            &ribbon.id[..ribbon.id.len().min(8)]
        );
        return Ok((
            Place {
                ribbon_id: ribbon.id.clone(),
                distance,
            },
            label,
        ));
    }
    let needle = spec.to_lowercase();
    let mut candidates: Vec<_> = network
        .markers
        .iter()
        .filter(|m| m.name.to_lowercase().starts_with(&needle))
        .collect();
    // An exact name wins; otherwise platforms before sidings, then by name.
    candidates.sort_by_key(|m| {
        (
            m.name.to_lowercase() != needle,
            !m.kind.to_lowercase().contains("platform"),
            m.name.clone(),
        )
    });
    let marker = candidates
        .first()
        .ok_or_else(|| format!("no marker starts with '{spec}'"))?;
    Ok((
        Place {
            ribbon_id: marker.ribbon_id.clone(),
            distance: (marker.start + marker.end) / 2.0,
        },
        marker.name.clone(),
    ))
}

fn path(root: &str, wanted: &str, from: &str, to: &str, output: Option<&str>) -> CliResult {
    use opentsc_route::graph::{Direction, TrackGraph};
    use opentsc_route::map::{render_svg_with_path, TrackMap};
    use std::collections::{HashMap, HashSet};

    let mut out = String::new();
    let route = find_route(root, wanted)?;
    let title = route.name.clone().unwrap_or_else(|| route.id.clone());
    let network = route.track_network()?;
    let tracks = TrackMap::build(&route, 0.15)?;
    let graph = TrackGraph::build(&network, &tracks.ribbon_shapes);
    let (start, start_name) = find_place(&network, from)?;
    let (goal, goal_name) = find_place(&network, to)?;
    let Some(found) = graph.shortest_path(&start, &goal) else {
        return Err(format!("no way for a train from {start_name} to {goal_name}").into());
    };
    outln!(out, "{title}: {start_name} -> {goal_name}");
    outln!(
        out,
        "  {:.3} km over {} ribbons",
        found.length / 1000.0,
        found.steps.len()
    );
    // Platforms passed on the way, once per station.
    let lengths: HashMap<&str, f64> = network
        .ribbons
        .iter()
        .map(|r| (r.id.as_str(), r.length))
        .collect();
    let mut seen = HashSet::new();
    let mut entered = 0.0; // metres travelled on entering the current ribbon
    for (i, step) in found.steps.iter().enumerate() {
        let length = lengths.get(step.ribbon_id.as_str()).copied().unwrap_or(0.0);
        let forward = step.direction == Direction::Forward;
        // Distance along the ribbon at which the train enters it.
        let entry = match (i, forward) {
            (0, _) => start.distance,
            (_, true) => 0.0,
            (_, false) => length,
        };
        let mut here: Vec<(f64, &str)> = network
            .markers
            .iter()
            .filter(|m| m.ribbon_id == step.ribbon_id && m.kind.to_lowercase().contains("platform"))
            .map(|m| {
                let middle = (m.start + m.end) / 2.0;
                (
                    entered
                        + if forward {
                            middle - entry
                        } else {
                            entry - middle
                        },
                    m.name.as_str(),
                )
            })
            .filter(|&(at, _)| (0.0..=found.length).contains(&at))
            .collect();
        here.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (at, name) in here {
            let station = name.split(" Spor").next().unwrap_or(name).trim();
            if seen.insert(station.to_string()) {
                outln!(out, "  {:8.1} km  {station}", at / 1000.0);
            }
        }
        entered += if forward { length - entry } else { entry };
    }
    match output {
        // A table of the ribbons travelled: id, direction, and the stretch
        // of each that the path uses.
        Some(output) if output.to_lowercase().ends_with(".tsv") => {
            let mut table = String::from("ribbon\tdirection\tfrom\tto\n");
            let last = found.steps.len() - 1;
            for (i, step) in found.steps.iter().enumerate() {
                let length = lengths.get(step.ribbon_id.as_str()).copied().unwrap_or(0.0);
                let forward = step.direction == Direction::Forward;
                let from = match (i, forward) {
                    (0, _) => start.distance,
                    (_, true) => 0.0,
                    (_, false) => length,
                };
                let to = match (i == last, forward) {
                    (true, _) => goal.distance,
                    (_, true) => length,
                    (_, false) => 0.0,
                };
                table.push_str(&format!(
                    "{}\t{}\t{from:.3}\t{to:.3}\n",
                    step.ribbon_id,
                    if forward { "forward" } else { "backward" }
                ));
            }
            fs::write(output, table)?;
            outln!(out, "  wrote {output}");
        }
        Some(output) => {
            let ids: Vec<String> = found.steps.iter().map(|s| s.ribbon_id.clone()).collect();
            let caption = vec![format!(
                "{start_name} to {goal_name}: {:.1} km",
                found.length / 1000.0
            )];
            fs::write(
                output,
                render_svg_with_path(&title, &caption, &tracks, &[], &ids),
            )?;
            outln!(out, "  wrote {output}");
        }
        None => {}
    }
    print_all(&out)
}

fn map(root: &str, wanted: &str, output: &str, with_terrain: bool) -> CliResult {
    use opentsc_route::map::{render_svg, terrain_heights, NodeCheck, TrackMap};

    let route = find_route(root, wanted)?;
    let title = route.name.clone().unwrap_or_else(|| route.id.clone());
    // Lines may stray 15 cm from the true curve: invisible on a map.
    let tracks = TrackMap::build(&route, 0.15)?;
    let network = route.track_network()?;
    let terrain = if with_terrain {
        terrain_heights(&route)?
    } else {
        Vec::new()
    };

    println!("{title}  ({})", route.id);
    println!(
        "  track tiles {:>7}    curves {} ({} straight, {} arc, {} easement)",
        tracks.tiles,
        tracks.curves(),
        tracks.straights,
        tracks.arcs,
        tracks.easements
    );
    if tracks.loose_curves > 0 {
        println!(
            "  {} of the curves belong to no ribbon",
            tracks.loose_curves
        );
    }
    for (name, count) in &tracks.unknown_curves {
        println!("  NOT READ: {count} curves of unknown kind {name}");
    }
    println!(
        "  ribbons     {:>7}    track length {:.1} km",
        network.ribbons.len(),
        tracks.length() / 1000.0
    );

    // Two independent checks of the geometry.
    let (mut agree, mut differ, mut without_curves) = (0, 0, 0);
    for ribbon in &network.ribbons {
        match tracks.ribbon_lengths.get(&ribbon.id) {
            Some(sum) if (sum - ribbon.length).abs() <= 0.05 => agree += 1,
            Some(_) => differ += 1,
            None => without_curves += 1,
        }
    }
    println!(
        "  ribbon lengths: {agree} match their curves within 5 cm, {differ} differ, \
         {without_curves} have no curves"
    );
    println!(
        "  joints between curves: {}; gap median {:.1} mm, 99% below {:.1} mm, worst {:.3} m",
        tracks.joint_gaps.len(),
        tracks.gap_quantile(0.5) * 1000.0,
        tracks.gap_quantile(0.99) * 1000.0,
        tracks.gap_quantile(1.0)
    );
    let nodes = NodeCheck::build(&network, &tracks);
    let [ends, joints, turnouts, crossings] = nodes.by_connections;
    println!(
        "  nodes       {:>7}    {ends} track ends, {joints} plain joints, {turnouts} with three \
         ends, {crossings} with four or more",
        network.nodes.len()
    );
    println!(
        "  ribbon ends meeting at a node: {} nodes checked; apart by median {:.1} mm, \
         99% below {:.1} mm, worst {:.3} m",
        nodes.spreads.len(),
        nodes.spread_quantile(0.5) * 1000.0,
        nodes.spread_quantile(0.99) * 1000.0,
        nodes.spread_quantile(1.0)
    );
    if !nodes.loose.is_empty() {
        println!(
            "  {} nodes have ends more than 5 cm apart; the widest:",
            nodes.loose.len()
        );
        for (spread, ends, pattern) in nodes.loose.iter().take(5) {
            println!(
                "    {spread:.3} m, {ends} ends, {}",
                pattern.as_deref().unwrap_or("no pattern")
            );
        }
    }
    for (id, gap) in tracks.broken_ribbons.iter().take(5) {
        println!("  ribbon {id} is not one unbroken line: gap of {gap:.1} m");
    }
    let mut caption = vec![format!(
        "{:.0} km of track, {} curves in {} tiles",
        tracks.length() / 1000.0,
        tracks.curves(),
        tracks.tiles
    )];
    if !terrain.is_empty() {
        let low = terrain.iter().map(|t| t.min).fold(f32::INFINITY, f32::min);
        let high = terrain
            .iter()
            .map(|t| t.max)
            .fold(f32::NEG_INFINITY, f32::max);
        println!(
            "  terrain tiles {:>5}    heights {low:.0} m to {high:.0} m",
            terrain.len()
        );
        caption.push(format!("terrain {low:.0} to {high:.0} m, shaded by tile"));
    }

    fs::write(output, render_svg(&title, &caption, &tracks, &terrain))?;
    println!("  wrote {output}");
    Ok(ExitCode::SUCCESS)
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
    /// Files that parsed but close an element under the wrong name.
    mismatched: Vec<String>,
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
        let outcome = if serz::is_serz(data) {
            serz::parse_with_report(data).map(|(_, report)| report)
        } else {
            parse_document(data).map(|_| serz::Report::default())
        };
        match outcome {
            Ok(report) => {
                counts.0 += 1;
                if report.mismatched_closes > 0 {
                    self.mismatched.push(label.to_string());
                }
            }
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
    if !scan.mismatched.is_empty() {
        println!(
            "\n{} files close an element under the wrong name (tolerated):",
            scan.mismatched.len()
        );
        for label in scan.mismatched.iter().take(scan.max_errors) {
            println!("  {label}");
        }
    }
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
