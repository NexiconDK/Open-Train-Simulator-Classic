//! A flat map of a route, written as SVG.
//!
//! This is a checking tool rather than a feature: drawing every curve of
//! every track tile from above shows at a glance whether the geometry is
//! being read correctly.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::geom::{Curve, CurveKind, Vec2};
use crate::graph::RibbonShape;
use crate::tile::{TileCoord, TILE_SIZE};
use crate::track::{assemble, RibbonEnd, TrackNetwork};
use crate::{Result, Route};

/// The id under which track tiles store curves that belong to no ribbon.
const NO_RIBBON: &str = "00000000-0000-0000-0000-000000000000";

/// Curve ends further apart than this are drawn as separate lines.
const JOIN_LIMIT: f64 = 1.0;

/// The track of a route as drawable lines, with figures about what was read.
#[derive(Debug, Clone, Default)]
pub struct TrackMap {
    /// Centre lines as point lists, in route metres.
    pub lines: Vec<Vec<Vec2>>,
    pub tiles: usize,
    pub straights: usize,
    pub arcs: usize,
    pub easements: usize,
    /// Class names of curves that could not be read, with how many of each.
    pub unknown_curves: HashMap<String, usize>,
    /// Summed curve length per ribbon id, in metres.
    pub ribbon_lengths: HashMap<String, f64>,
    /// Where each ribbon starts and ends, in route metres.
    pub ribbon_ends: HashMap<String, (Vec2, Vec2)>,
    /// Start pose, end pose and length of each ribbon, for route finding.
    pub ribbon_shapes: HashMap<String, RibbonShape>,
    /// Which entries of `lines` draw each ribbon.
    pub ribbon_lines: HashMap<String, Vec<usize>>,
    /// Ribbons whose curves do not form one unbroken line, with the widest
    /// gap in metres.
    pub broken_ribbons: Vec<(String, f64)>,
    /// Curves that belong to no ribbon.
    pub loose_curves: usize,
    /// Length of all curves together, in metres.
    pub total_length: f64,
    /// Distance between the computed end of each curve and the start of the
    /// next one of the same ribbon, in metres. All zeros would be perfect.
    pub joint_gaps: Vec<f64>,
}

impl TrackMap {
    /// Read every track tile of `route`. `tolerance` is how far, in metres,
    /// a drawn line may stray from the true curve.
    pub fn build(route: &Route, tolerance: f64) -> Result<Self> {
        let mut map = TrackMap::default();
        // A ribbon's curves are spread over several tiles: gather first.
        let mut order: Vec<String> = Vec::new();
        let mut by_ribbon: HashMap<String, Vec<Curve>> = HashMap::new();
        for coord in route.track_tile_coords()? {
            let tile = route.track_tile(coord)?;
            map.tiles += 1;
            for name in tile.unknown_curves {
                *map.unknown_curves.entry(name).or_default() += 1;
            }
            for mut ribbon in tile.ribbons {
                // Curves without a ribbon (stored under the all-zero id) are
                // unrelated pieces; grouping them would chain them at random.
                let key = if ribbon.ribbon_id == NO_RIBBON {
                    map.loose_curves += ribbon.curves.len();
                    format!("{NO_RIBBON}#{}", order.len())
                } else {
                    ribbon.ribbon_id.clone()
                };
                let curves = by_ribbon.entry(key.clone()).or_insert_with(|| {
                    order.push(key);
                    Vec::new()
                });
                curves.append(&mut ribbon.curves);
            }
        }

        for id in order {
            let (curves, gaps) = assemble(by_ribbon.remove(&id).unwrap_or_default());
            let length: f64 = curves.iter().map(|c| c.length).sum();
            map.total_length += length;
            // Ribbon-less pieces are not expected to join up, so they stay
            // out of the joint figures.
            let is_ribbon = !id.starts_with(NO_RIBBON);
            if is_ribbon {
                if let Some(&widest) = gaps.iter().max_by(|a, b| a.total_cmp(b)) {
                    if widest > JOIN_LIMIT {
                        map.broken_ribbons.push((id.clone(), widest));
                    }
                }
                map.joint_gaps.extend(gaps.iter().copied());
                if let (Some(first), Some(last)) = (curves.first(), curves.last()) {
                    let (start, end) = (first.pose_at(0.0), last.end());
                    map.ribbon_ends
                        .insert(id.clone(), (start.position, end.position));
                    map.ribbon_shapes
                        .insert(id.clone(), RibbonShape { start, end, length });
                }
                map.ribbon_lengths.insert(id.clone(), length);
            }
            let first_line = map.lines.len();
            let mut line: Vec<Vec2> = Vec::new();
            for (i, curve) in curves.iter().enumerate() {
                match curve.kind {
                    CurveKind::Straight => map.straights += 1,
                    CurveKind::Arc { .. } => map.arcs += 1,
                    CurveKind::Easement(_) => map.easements += 1,
                }
                let mut points = curve.polyline(tolerance);
                if i > 0 {
                    if gaps[i - 1] > JOIN_LIMIT {
                        map.lines.push(std::mem::take(&mut line));
                    } else {
                        points.remove(0); // the previous curve ended here
                    }
                }
                line.append(&mut points);
            }
            if line.len() > 1 {
                map.lines.push(line);
            }
            if is_ribbon {
                map.ribbon_lines
                    .insert(id, (first_line..map.lines.len()).collect());
            }
        }
        map.joint_gaps.sort_by(f64::total_cmp);
        Ok(map)
    }

    pub fn curves(&self) -> usize {
        self.straights + self.arcs + self.easements
    }

    /// Total length of all curves in metres.
    pub fn length(&self) -> f64 {
        self.total_length
    }

    /// The gap that `fraction` of all joints stay below (0.5 = median).
    pub fn gap_quantile(&self, fraction: f64) -> f64 {
        match self.joint_gaps.len() {
            0 => 0.0,
            n => self.joint_gaps[((n - 1) as f64 * fraction.clamp(0.0, 1.0)).round() as usize],
        }
    }

    /// Smallest box around all lines: `[min_x, min_z, max_x, max_z]`.
    pub fn bounds(&self) -> Option<[f64; 4]> {
        let mut points = self.lines.iter().flatten();
        let first = points.next()?;
        Some(
            points.fold([first[0], first[1], first[0], first[1]], |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            }),
        )
    }
}

/// What the nodes of a network look like once ribbon geometry is known.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeCheck {
    /// How many nodes join 1, 2, 3 and 4 or more ribbon ends:
    /// track ends, plain joints, turnouts, crossings and slips.
    pub by_connections: [usize; 4],
    /// Nodes with no connection at all.
    pub empty: usize,
    /// For every node with two or more located ends, the largest distance
    /// in metres between any two of them, sorted. All zeros would be
    /// perfect: the ends of joined ribbons are in the same place.
    pub spreads: Vec<f64>,
    /// Nodes whose ends are more than 5 cm apart: the spread in metres, the
    /// number of connections and the junction pattern. Widest first.
    pub loose: Vec<(f64, usize, Option<String>)>,
    /// Connections to ribbons that have no geometry.
    pub unlocated: usize,
    /// Positions of the nodes that join three or more ends.
    pub junctions: Vec<Vec2>,
}

impl NodeCheck {
    /// Compare every node of `network` with the ribbon ends in `tracks`.
    pub fn build(network: &TrackNetwork, tracks: &TrackMap) -> Self {
        let mut check = NodeCheck::default();
        for node in &network.nodes {
            match node.connections.len() {
                0 => check.empty += 1,
                n => check.by_connections[n.min(4) - 1] += 1,
            }
            let mut points: Vec<Vec2> = Vec::new();
            for link in &node.connections {
                match tracks.ribbon_ends.get(&link.ribbon_id) {
                    Some(&(start, end)) => points.push(match link.end {
                        RibbonEnd::Start => start,
                        RibbonEnd::End => end,
                    }),
                    None => check.unlocated += 1,
                }
            }
            if points.len() >= 2 {
                let mut spread = 0.0f64;
                for (i, a) in points.iter().enumerate() {
                    for b in &points[i + 1..] {
                        spread = spread.max((a[0] - b[0]).hypot(a[1] - b[1]));
                    }
                }
                check.spreads.push(spread);
                if spread > 0.05 {
                    check
                        .loose
                        .push((spread, node.connections.len(), node.pattern.clone()));
                }
            }
            if node.connections.len() >= 3 {
                if let Some(&point) = points.first() {
                    check.junctions.push(point);
                }
            }
        }
        check.spreads.sort_by(f64::total_cmp);
        check.loose.sort_by(|a, b| b.0.total_cmp(&a.0));
        check
    }

    /// The spread that `fraction` of the nodes stay below (0.5 = median).
    pub fn spread_quantile(&self, fraction: f64) -> f64 {
        match self.spreads.len() {
            0 => 0.0,
            n => self.spreads[((n - 1) as f64 * fraction.clamp(0.0, 1.0)).round() as usize],
        }
    }
}

/// Average height of one terrain tile, for shading the map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileHeight {
    pub coord: TileCoord,
    pub mean: f32,
    pub min: f32,
    pub max: f32,
}

/// Read every terrain tile of `route` and keep its height figures.
pub fn terrain_heights(route: &Route) -> Result<Vec<TileHeight>> {
    let mut heights = Vec::new();
    for coord in route.terrain_tile_coords()? {
        let tile = route.terrain_tile(coord)?;
        let (min, max) = tile.range();
        heights.push(TileHeight {
            coord,
            mean: tile.mean(),
            min,
            max,
        });
    }
    Ok(heights)
}

/// Draw the map. `caption` lines go into the legend under the title.
pub fn render_svg(
    title: &str,
    caption: &[String],
    tracks: &TrackMap,
    terrain: &[TileHeight],
) -> String {
    render_svg_with_path(title, caption, tracks, terrain, &[])
}

/// Like [`render_svg`], with the ribbons listed in `path` drawn on top in a
/// second colour.
pub fn render_svg_with_path(
    title: &str,
    caption: &[String],
    tracks: &TrackMap,
    terrain: &[TileHeight],
    path: &[String],
) -> String {
    let Some([min_x, min_z, max_x, max_z]) = tracks.bounds() else {
        return String::from(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 400 60\">\
             <text x=\"10\" y=\"35\" font-family=\"sans-serif\">This route has no track.</text></svg>\n",
        );
    };
    let margin = (0.03 * (max_x - min_x).max(max_z - min_z)).max(150.0);
    let (left, top) = (min_x - margin, max_z + margin);
    let width = max_x - min_x + 2.0 * margin;
    let height = max_z - min_z + 2.0 * margin;
    // SVG's y axis points down, north should point up: y = top - z.
    let pixel_width = 1600.0;
    let pixel_height = (pixel_width * height / width).clamp(300.0, 6000.0);

    let mut svg = String::with_capacity(tracks.lines.len() * 96);
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width:.0} {height:.0}\" \
         width=\"{pixel_width:.0}\" height=\"{pixel_height:.0}\">"
    );
    let _ = writeln!(
        svg,
        "<style>.tracks path{{vector-effect:non-scaling-stroke}} \
         text{{font-family:system-ui,'Segoe UI',sans-serif;fill:#1f2328}}</style>"
    );
    let _ = writeln!(
        svg,
        "<rect width=\"100%\" height=\"100%\" fill=\"#f6f5f1\"/>"
    );

    // Terrain: one square per tile, tinted by its mean height.
    let visible: Vec<&TileHeight> = terrain
        .iter()
        .filter(|t| {
            let [x, z] = t.coord.origin();
            x + TILE_SIZE > left && x < left + width && z < top && z + TILE_SIZE > top - height
        })
        .collect();
    if !visible.is_empty() {
        // Stretch the colours over the middle 96% of tiles, so that a few
        // empty (0 m) or mountain-top tiles do not flatten everything else.
        let mut means: Vec<f32> = visible.iter().map(|t| t.mean).collect();
        means.sort_by(f32::total_cmp);
        let at = |fraction: f32| means[((means.len() - 1) as f32 * fraction).round() as usize];
        let (low, high) = (at(0.02), at(0.98));
        let span = (high - low).max(1.0);
        let _ = writeln!(svg, "<g shape-rendering=\"crispEdges\">");
        for tile in visible {
            let [x, z] = tile.coord.origin();
            let _ = writeln!(
                svg,
                "<rect x=\"{:.0}\" y=\"{:.0}\" width=\"{TILE_SIZE:.0}\" height=\"{TILE_SIZE:.0}\" fill=\"{}\"/>",
                x - left,
                top - (z + TILE_SIZE),
                height_colour((tile.mean - low) / span)
            );
        }
        let _ = writeln!(svg, "</g>");
    }

    // Track centre lines. One decimal is 10 cm, plenty for a map.
    let _ = writeln!(
        svg,
        "<g class=\"tracks\" fill=\"none\" stroke=\"#20242b\" stroke-width=\"1.4\" \
         stroke-linejoin=\"round\" stroke-linecap=\"round\">"
    );
    for line in &tracks.lines {
        svg.push_str("<path d=\"");
        for (i, point) in line.iter().enumerate() {
            let _ = write!(
                svg,
                "{}{:.1} {:.1}",
                if i == 0 { "M" } else { "L" },
                point[0] - left,
                top - point[1]
            );
        }
        svg.push_str("\"/>\n");
    }
    let _ = writeln!(svg, "</g>");

    if !path.is_empty() {
        let _ = writeln!(
            svg,
            "<g class=\"tracks\" fill=\"none\" stroke=\"#d1242f\" stroke-width=\"3\" \
             stroke-linejoin=\"round\" stroke-linecap=\"round\">"
        );
        let lines = path
            .iter()
            .filter_map(|id| tracks.ribbon_lines.get(id))
            .flatten()
            .filter_map(|&i| tracks.lines.get(i));
        for line in lines {
            svg.push_str("<path d=\"");
            for (i, point) in line.iter().enumerate() {
                let _ = write!(
                    svg,
                    "{}{:.1} {:.1}",
                    if i == 0 { "M" } else { "L" },
                    point[0] - left,
                    top - point[1]
                );
            }
            svg.push_str("\"/>\n");
        }
        let _ = writeln!(svg, "</g>");
    }

    // Legend: title, caption, scale bar. Sized relative to the map.
    let unit = width.min(height * 2.0) / 60.0;
    let pad = unit * 0.8;
    let box_height = pad * 2.0 + unit * 1.5 + unit * 1.25 * caption.len() as f64 + unit * 1.8;
    let longest = caption
        .iter()
        .map(|c| c.chars().count())
        .chain([title.chars().count() * 3 / 2])
        .max();
    let box_width = (longest.unwrap_or(20) as f64 * unit * 0.56 + pad * 2.0).max(unit * 16.0);
    // Put the legend in the corner where it hides the least track.
    let corners = [
        (pad, pad),
        (width - pad - box_width, pad),
        (pad, height - pad - box_height),
        (width - pad - box_width, height - pad - box_height),
    ];
    let hidden = |&(x, y): &(f64, f64)| {
        tracks
            .lines
            .iter()
            .flatten()
            .filter(|p| {
                let (px, py) = (p[0] - left, top - p[1]);
                px >= x && px <= x + box_width && py >= y && py <= y + box_height
            })
            .count()
    };
    let (box_x, box_y) = corners.into_iter().min_by_key(hidden).unwrap_or((pad, pad));
    let _ = writeln!(
        svg,
        "<rect x=\"{box_x:.0}\" y=\"{box_y:.0}\" width=\"{box_width:.0}\" height=\"{box_height:.0}\" \
         rx=\"{:.0}\" fill=\"#ffffff\" fill-opacity=\"0.88\" stroke=\"#c8c6be\" stroke-width=\"{:.1}\"/>",
        unit * 0.4,
        unit * 0.06
    );
    let text_x = box_x + pad;
    let mut y = box_y + pad + unit * 1.2;
    let _ = writeln!(
        svg,
        "<text x=\"{text_x:.0}\" y=\"{y:.0}\" font-size=\"{:.0}\" font-weight=\"600\">{}</text>",
        unit * 1.5,
        escape(title)
    );
    for line in caption {
        y += unit * 1.25;
        let _ = writeln!(
            svg,
            "<text x=\"{text_x:.0}\" y=\"{y:.0}\" font-size=\"{unit:.0}\" fill-opacity=\"0.8\">{}</text>",
            escape(line)
        );
    }
    let bar = scale_bar_length(box_width * 0.6);
    y += unit * 1.3;
    let _ = writeln!(
        svg,
        "<path d=\"M{text_x:.0} {y:.0}h{bar:.0}\" stroke=\"#1f2328\" stroke-width=\"{:.1}\"/>\
         <text x=\"{:.0}\" y=\"{:.0}\" font-size=\"{unit:.0}\">{}</text>",
        unit * 0.25,
        text_x + bar + unit * 0.6,
        y + unit * 0.35,
        if bar >= 1000.0 {
            format!("{:.0} km", bar / 1000.0)
        } else {
            format!("{bar:.0} m")
        }
    );
    svg.push_str("</svg>\n");
    svg
}

/// Largest of 1, 2 or 5 times a power of ten that fits in `available` metres.
fn scale_bar_length(available: f64) -> f64 {
    let magnitude = 10f64.powf(available.max(1.0).log10().floor());
    [5.0, 2.0, 1.0]
        .iter()
        .map(|m| m * magnitude)
        .find(|&length| length <= available)
        .unwrap_or(magnitude)
}

/// Lowland green through pale yellow to upland brown; `t` runs 0 to 1.
fn height_colour(t: f32) -> String {
    const STOPS: [[f32; 3]; 3] = [
        [207.0, 226.0, 196.0],
        [238.0, 230.0, 190.0],
        [203.0, 168.0, 132.0],
    ];
    let t = t.clamp(0.0, 1.0) * 2.0;
    let (from, to, f) = if t < 1.0 {
        (STOPS[0], STOPS[1], t)
    } else {
        (STOPS[1], STOPS[2], t - 1.0)
    };
    let channel = |i: usize| (from[i] + (to[i] - from[i]) * f).round() as u8;
    format!("#{:02x}{:02x}{:02x}", channel(0), channel(1), channel(2))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_bar_picks_round_lengths() {
        assert_eq!(scale_bar_length(7300.0), 5000.0);
        assert_eq!(scale_bar_length(4999.0), 2000.0);
        assert_eq!(scale_bar_length(1500.0), 1000.0);
        assert_eq!(scale_bar_length(180.0), 100.0);
    }

    #[test]
    fn height_colours_run_from_green_to_brown() {
        assert_eq!(height_colour(0.0), "#cfe2c4");
        assert_eq!(height_colour(0.5), "#eee6be");
        assert_eq!(height_colour(1.0), "#cba884");
        assert_eq!(height_colour(7.0), "#cba884");
    }

    #[test]
    fn draws_lines_with_north_up() {
        let tracks = TrackMap {
            lines: vec![
                vec![[0.0, 0.0], [1000.0, 0.0]],
                vec![[1000.0, 0.0], [1000.0, 2000.0]],
            ],
            ..Default::default()
        };
        let terrain = [TileHeight {
            coord: TileCoord { x: 0, z: 0 },
            mean: 10.0,
            min: 5.0,
            max: 20.0,
        }];
        let svg = render_svg("Test & <route>", &["2 curves".into()], &tracks, &terrain);
        assert!(svg.contains("Test &amp; &lt;route&gt;"));
        assert_eq!(
            svg.matches("<path d=\"M").count(),
            3,
            "two tracks and the scale bar"
        );
        // Margin is 150 m: the southern end (z = 0) is drawn below the
        // northern end (z = 2000), i.e. with the larger y.
        assert!(svg.contains("M1150.0 2150.0L1150.0 150.0"));
        assert!(svg.contains("<rect x=\"150\" y=\"1126\" width=\"1024\""));
        assert_eq!(tracks.bounds(), Some([0.0, 0.0, 1000.0, 2000.0]));
    }

    #[test]
    fn empty_route_still_gives_valid_svg() {
        let svg = render_svg("x", &[], &TrackMap::default(), &[]);
        assert!(svg.starts_with("<svg") && svg.contains("no track"));
    }
}
