//! The track network: ribbons and the curves they are made of.
//!
//! A *ribbon* is one stretch of track between two junctions or track ends.
//! `Networks/Tracks.bin` lists every ribbon with its length, height profile
//! and properties. The centre-line geometry is stored separately, per tile,
//! in `Networks/Track Tiles`.

use opentsc_formats::{Document, Element};

use crate::geom::{Curve, CurveKind, Easement, Vec2};
use crate::tile::TILE_SIZE;
use crate::{Error, Result};

/// The curves of one ribbon found in one track tile, in order of travel.
#[derive(Debug, Clone, PartialEq)]
pub struct RibbonCurves {
    /// The ribbon's GUID as text, e.g. `0372879b-33e0-4499-94a4-c99d679ac4b1`.
    pub ribbon_id: String,
    pub curves: Vec<Curve>,
}

/// Content of one file in `Networks/Track Tiles`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackTile {
    pub ribbons: Vec<RibbonCurves>,
    /// Class names of curves this reader does not understand. They are left
    /// out of `ribbons`.
    pub unknown_curves: Vec<String>,
}

impl TrackTile {
    pub fn parse(doc: &Document) -> Result<Self> {
        let record = record(doc)?;
        let mut tile = TrackTile::default();
        for container in record.elements() {
            let Some(curves) = container.child("Curve") else {
                continue;
            };
            let ribbon_id = container
                .child("RibbonID")
                .and_then(|id| id.find("cGUID"))
                .and_then(|guid| guid.string("DevString"))
                .ok_or_else(|| Error::data("curve container without a ribbon id"))?
                .to_string();
            let mut parsed = Vec::new();
            for curve in curves.elements() {
                match parse_curve(curve)? {
                    Some(curve) => parsed.push(curve),
                    None => tile.unknown_curves.push(curve.name.clone()),
                }
            }
            tile.ribbons.push(RibbonCurves {
                ribbon_id,
                curves: parsed,
            });
        }
        Ok(tile)
    }
}

fn parse_curve(e: &Element) -> Result<Option<Curve>> {
    let kind = match e.name.as_str() {
        "cCurveStraight" => CurveKind::Straight,
        // CurveSign +1 is a right-hand bend; headings here count left-positive.
        "cCurveArc" => CurveKind::Arc {
            curvature: -number(e, "Curvature")? * number(e, "CurveSign")?,
        },
        "cCurveEasement" => CurveKind::Easement(Easement {
            sharpness: number(e, "Sharpness")?,
            offset: number(e, "Offset")?,
            l0: number(e, "L0")?,
            curve_sign: number(e, "CurveSign")?,
            traversal_sign: number(e, "TraversalSign")?,
        }),
        _ => return Ok(None),
    };
    let start = e
        .path(&["StartPos", "cFarVector2"])
        .ok_or_else(|| Error::data(format!("{} without StartPos", e.name)))?;
    let tangent = e
        .array("StartTangent")
        .filter(|t| t.len() == 2)
        .and_then(|t| Some([t[0].as_f64()?, t[1].as_f64()?]))
        .ok_or_else(|| Error::data(format!("{} without StartTangent", e.name)))?;
    Ok(Some(Curve {
        start: far_vector(start)?,
        tangent,
        length: number(e, "Length")?,
        kind,
    }))
}

/// Curves further apart than this are not treated as joined.
const JOIN_LIMIT: f64 = 1.0;

/// Put the curves of one ribbon into order of travel.
///
/// A ribbon's curves are spread over the track tiles it passes through, and
/// a tile lists only the curves that start inside it, so the pieces have to
/// be matched up by position: each curve is followed by the one that starts
/// where it ends.
///
/// Returns the ordered curves and, for each joint between neighbours, the
/// distance in metres between the end of one and the start of the next.
/// Joints wider than a metre mean a piece is missing or was not understood.
pub fn assemble(curves: Vec<Curve>) -> (Vec<Curve>, Vec<f64>) {
    let count = curves.len();
    if count < 2 {
        return (curves, Vec::new());
    }
    let ends: Vec<Vec2> = curves.iter().map(|c| c.end().position).collect();
    let gap = |from: usize, to: usize| -> f64 {
        let (a, b) = (ends[from], curves[to].start);
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
    };

    // Candidate links, closest first; each curve gets one successor and one
    // predecessor at most.
    let mut links: Vec<(f64, usize, usize)> = Vec::new();
    for from in 0..count {
        for to in 0..count {
            if from != to {
                let distance = gap(from, to);
                if distance <= JOIN_LIMIT {
                    links.push((distance, from, to));
                }
            }
        }
    }
    links.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut next: Vec<Option<usize>> = vec![None; count];
    let mut has_previous = vec![false; count];
    for (_, from, to) in links {
        if next[from].is_none() && !has_previous[to] {
            next[from] = Some(to);
            has_previous[to] = true;
        }
    }

    // Walk each chain from its head. A ribbon that closes on itself has no
    // head; whatever is left over is walked from its lowest index.
    let mut order = Vec::with_capacity(count);
    let mut placed = vec![false; count];
    let heads = (0..count).filter(|&i| !has_previous[i]).chain(0..count);
    for head in heads {
        let mut at = Some(head);
        while let Some(i) = at.filter(|&i| !placed[i]) {
            placed[i] = true;
            order.push(i);
            at = next[i];
        }
    }

    let gaps = order.windows(2).map(|pair| gap(pair[0], pair[1])).collect();
    (order.into_iter().map(|i| curves[i]).collect(), gaps)
}

/// One ribbon as listed in `Networks/Tracks.bin`.
#[derive(Debug, Clone, PartialEq)]
pub struct Ribbon {
    pub id: String,
    /// Length in metres, in the same measure as [`Curve::length`].
    pub length: f64,
    /// Height profile along the ribbon, ordered by position.
    pub heights: Vec<HeightPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeightPoint {
    /// Distance from the start of the ribbon, in metres.
    pub position: f64,
    /// Height above the route's zero level, in metres.
    pub height: f64,
    /// Set by hand in the editor rather than interpolated.
    pub manual: bool,
}

impl Ribbon {
    /// Rail height at `position` metres along the ribbon, interpolated
    /// linearly between height points. `None` if the ribbon has no profile.
    pub fn height_at(&self, position: f64) -> Option<f64> {
        let (first, last) = (self.heights.first()?, self.heights.last()?);
        if position <= first.position {
            return Some(first.height);
        }
        if position >= last.position {
            return Some(last.height);
        }
        let after = self.heights.partition_point(|h| h.position <= position);
        let (a, b) = (self.heights[after - 1], self.heights[after]);
        let span = b.position - a.position;
        if span <= 0.0 {
            return Some(b.height);
        }
        Some(a.height + (b.height - a.height) * (position - a.position) / span)
    }
}

/// Which end of a ribbon a node is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RibbonEnd {
    /// Position 0: where the ribbon's first curve starts.
    Start,
    /// Position 1: where its last curve ends.
    End,
}

/// One ribbon end meeting a node.
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub ribbon_id: String,
    pub end: RibbonEnd,
}

/// A point where ribbon ends meet: a plain joint (two ends), a turnout
/// (three), a crossing or slip (four), or the end of the line (one).
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub connections: Vec<Connection>,
    /// Class name of the junction pattern, such as
    /// `Network::cTurnoutPattern`, if the node has one.
    pub pattern: Option<String>,
}

/// A named stretch of track: a platform, a siding or a destination.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// The name shown to the player, e.g. `Frederikshavn Spor 2`.
    pub name: String,
    /// File name of the marker's blueprint without its extension, which
    /// tells the kinds apart, e.g. `Platform marker`.
    pub kind: String,
    pub ribbon_id: String,
    /// Where the marked stretch starts and ends, in metres along the ribbon.
    pub start: f64,
    pub end: f64,
}

/// Content of `Networks/Tracks.bin`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackNetwork {
    pub ribbons: Vec<Ribbon>,
    pub nodes: Vec<Node>,
    pub markers: Vec<Marker>,
}

/// Read ribbons and nodes from `Networks/Tracks.bin`.
pub fn parse_track_network(doc: &Document) -> Result<TrackNetwork> {
    let network = doc
        .root()
        .and_then(|root| root.find("Network::cTrackNetwork"))
        .ok_or_else(|| Error::data("no Network::cTrackNetwork in the file"))?;
    let Some(container) = network
        .child("RibbonContainer")
        .and_then(|container| container.elements().next())
    else {
        return Ok(TrackNetwork::default());
    };

    let guid = |holder: Option<&Element>| -> Option<String> {
        holder?
            .find("cGUID")?
            .string("DevString")
            .map(str::to_string)
    };

    let mut ribbons = Vec::new();
    let mut markers = Vec::new();
    for ribbon in container
        .child("Ribbon")
        .into_iter()
        .flat_map(Element::elements)
    {
        let id =
            guid(ribbon.child("RibbonID")).ok_or_else(|| Error::data("ribbon without an id"))?;
        let mut heights = Vec::new();
        for point in ribbon
            .child("Height")
            .into_iter()
            .flat_map(Element::elements)
        {
            heights.push(HeightPoint {
                position: number(point, "_position")?,
                height: number(point, "_height")?,
                manual: point
                    .value("_manual")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            });
        }
        heights.sort_by(|a, b| a.position.total_cmp(&b.position));

        let properties = ribbon
            .child("Properties")
            .and_then(|p| p.elements().next())
            .and_then(|container| container.child("Property"));
        for property in properties.into_iter().flat_map(Element::elements) {
            if !is_class(property, "Network::cTrackMarkerProperty") {
                continue;
            }
            let Some(name) = property
                .find("Network::cTrackMarkerComponent")
                .and_then(|component| component.string("DisplayName"))
            else {
                continue;
            };
            let kind = property
                .path(&["Entity", "cOwnedEntity", "BlueprintID"])
                .and_then(|id| id.find("iBlueprintLibrary::cAbsoluteBlueprintID"))
                .and_then(|id| id.string("BlueprintID"))
                .map(|path| {
                    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
                    file.rsplit_once('.')
                        .map_or(file, |(stem, _)| stem)
                        .to_string()
                })
                .unwrap_or_default();
            markers.push(Marker {
                name: name.to_string(),
                kind,
                ribbon_id: id.clone(),
                start: number(property, "_start")?,
                end: number(property, "_end")?,
            });
        }
        ribbons.push(Ribbon {
            id,
            length: number(ribbon, "_length")?,
            heights,
        });
    }

    let mut nodes = Vec::new();
    for node in container
        .child("Node")
        .into_iter()
        .flat_map(Element::elements)
    {
        let mut connections = Vec::new();
        for link in node
            .child("Connection")
            .into_iter()
            .flat_map(Element::elements)
        {
            let ribbon_id = guid(link.child("_id"))
                .ok_or_else(|| Error::data("node connection without a ribbon id"))?;
            let position = link
                .path(&["_end", "cNormFloat"])
                .map(|p| number(p, "Position"))
                .transpose()?
                .ok_or_else(|| Error::data("node connection without an end"))?;
            let end = if position < 0.5 {
                RibbonEnd::Start
            } else {
                RibbonEnd::End
            };
            connections.push(Connection { ribbon_id, end });
        }
        let pattern = node
            .child("PatternRef")
            .and_then(|r| r.find("Pattern"))
            .and_then(|p| p.elements().next())
            .map(|p| p.name.clone());
        nodes.push(Node {
            connections,
            pattern,
        });
    }
    Ok(TrackNetwork {
        ribbons,
        nodes,
        markers,
    })
}

/// True if `e` is an element of class `name` (either spelling of `::`).
pub(crate) fn is_class(e: &Element, name: &str) -> bool {
    opentsc_formats::node::name_eq(&e.name, name)
}

/// The `Record` element every route data file wraps its content in.
pub(crate) fn record(doc: &Document) -> Result<&Element> {
    doc.root()
        .and_then(|root| root.child("Record"))
        .ok_or_else(|| Error::data("not a record set"))
}

pub(crate) fn number(e: &Element, name: &str) -> Result<f64> {
    e.value(name)
        .and_then(|v| v.as_f64())
        .ok_or_else(|| Error::data(format!("{} without a numeric {name}", e.name)))
}

/// Read a `cFarVector2`: per axis a whole tile count plus metres inside the
/// tile. Keeping the two apart is how the game stays precise far from the
/// origin; here they are combined into `f64` metres.
pub(crate) fn far_vector(v: &Element) -> Result<Vec2> {
    let axis = |name: &str| -> Result<f64> {
        let coordinate = v
            .path(&[name, "cFarCoordinate"])
            .ok_or_else(|| Error::data(format!("far vector without {name}")))?;
        let tiles = coordinate
            .path(&["RouteCoordinate", "cRouteCoordinate"])
            .map(|c| number(c, "Distance"))
            .transpose()?
            .ok_or_else(|| Error::data("far coordinate without a route part"))?;
        let metres = coordinate
            .path(&["TileCoordinate", "cTileCoordinate"])
            .map(|c| number(c, "Distance"))
            .transpose()?
            .ok_or_else(|| Error::data("far coordinate without a tile part"))?;
        Ok(tiles * TILE_SIZE + metres)
    };
    Ok([axis("X")?, axis("Z")?])
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentsc_formats::xml;

    const D: &str = r#"xmlns:d="http://www.kuju.com/TnT/2003/Delta""#;

    fn far(x_tile: i32, x: f32, z_tile: i32, z: f32) -> String {
        let axis = |name: &str, tile: i32, metres: f32| {
            format!(
                "<{name}><cFarCoordinate>\
                 <RouteCoordinate><cRouteCoordinate><Distance d:type=\"sInt32\">{tile}</Distance></cRouteCoordinate></RouteCoordinate>\
                 <TileCoordinate><cTileCoordinate><Distance d:type=\"sFloat32\">{metres}</Distance></cTileCoordinate></TileCoordinate>\
                 </cFarCoordinate></{name}>"
            )
        };
        format!(
            "<StartPos><cFarVector2>{}{}</cFarVector2></StartPos>",
            axis("X", x_tile, x),
            axis("Z", z_tile, z)
        )
    }

    #[test]
    fn reads_all_three_curve_kinds() {
        let text = format!(
            r#"<cRecordSet {D}><Record>
            <Network-cNetworkRibbonUnstreamed-cCurveContainerUnstreamed>
              <RibbonID><Network-cNetworkRibbon-cCurveContainer-cID>
                <RibbonID><cGUID><DevString d:type="cDeltaString">ribbon-a</DevString></cGUID></RibbonID>
                <NetworkTypeID><cGUID><DevString d:type="cDeltaString">type</DevString></cGUID></NetworkTypeID>
              </Network-cNetworkRibbon-cCurveContainer-cID></RibbonID>
              <Curve>
                <cCurveStraight>
                  <Length d:type="sFloat32">100</Length>{start}
                  <StartTangent d:numElements="2" d:elementType="sFloat32">1 0</StartTangent>
                </cCurveStraight>
                <cCurveArc>
                  <Curvature d:type="sFloat32">0.002</Curvature>
                  <Length d:type="sFloat32">50</Length>{start}
                  <StartTangent d:numElements="2" d:elementType="sFloat32">1 0</StartTangent>
                  <CurveSign d:type="sFloat32">1</CurveSign>
                </cCurveArc>
                <cCurveEasement>
                  <Length d:type="sFloat32">30</Length>{start}
                  <StartTangent d:numElements="2" d:elementType="sFloat32">0 1</StartTangent>
                  <Sharpness d:type="sFloat32">0.0000335</Sharpness>
                  <Offset d:type="sFloat32">-4</Offset>
                  <OffsetIsZero d:type="bool">0</OffsetIsZero>
                  <L0 d:type="sFloat32">12.5</L0>
                  <CurveSign d:type="sFloat32">-1</CurveSign>
                  <TraversalSign d:type="sFloat32">-1</TraversalSign>
                </cCurveEasement>
                <cCurveFromTheFuture><Length d:type="sFloat32">1</Length></cCurveFromTheFuture>
              </Curve>
            </Network-cNetworkRibbonUnstreamed-cCurveContainerUnstreamed>
            </Record></cRecordSet>"#,
            start = far(2, 24.0, -3, 1000.0)
        );
        let tile = TrackTile::parse(&xml::parse_str(&text).unwrap()).unwrap();
        assert_eq!(tile.unknown_curves, ["cCurveFromTheFuture"]);
        assert_eq!(tile.ribbons.len(), 1);
        let ribbon = &tile.ribbons[0];
        assert_eq!(ribbon.ribbon_id, "ribbon-a");
        assert_eq!(ribbon.curves.len(), 3);
        // Two tiles east plus 24 m; three tiles south plus 1000 m.
        assert_eq!(
            ribbon.curves[0].start,
            [2.0 * 1024.0 + 24.0, -3.0 * 1024.0 + 1000.0]
        );
        assert_eq!(ribbon.curves[0].kind, CurveKind::Straight);
        // A right-hand bend is a negative (clockwise) curvature.
        match ribbon.curves[1].kind {
            CurveKind::Arc { curvature } => assert!((curvature + 0.002).abs() < 1e-9),
            other => panic!("{other:?}"),
        }
        match ribbon.curves[2].kind {
            CurveKind::Easement(e) => {
                assert_eq!(
                    (e.offset, e.curve_sign, e.traversal_sign),
                    (-4.0, -1.0, -1.0)
                );
                assert_eq!(e.l0, 12.5);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn assembles_shuffled_curves_in_travel_order() {
        // Four straights of 100 m end to end along +x, handed over shuffled.
        let piece = |i: usize| Curve {
            start: [100.0 * i as f64, 7.0],
            tangent: [1.0, 0.0],
            length: 100.0,
            kind: CurveKind::Straight,
        };
        let (ordered, gaps) = assemble(vec![piece(2), piece(0), piece(3), piece(1)]);
        let starts: Vec<f64> = ordered.iter().map(|c| c.start[0]).collect();
        assert_eq!(starts, [0.0, 100.0, 200.0, 300.0]);
        assert_eq!(gaps, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_missing_piece_shows_up_as_a_wide_gap() {
        let piece = |i: usize| Curve {
            start: [100.0 * i as f64, 0.0],
            tangent: [1.0, 0.0],
            length: 100.0,
            kind: CurveKind::Straight,
        };
        let (ordered, gaps) = assemble(vec![piece(3), piece(0), piece(1)]);
        assert_eq!(ordered.len(), 3);
        assert_eq!(gaps.iter().filter(|g| **g > 1.0).count(), 1);
        assert!(assemble(vec![piece(0)]).1.is_empty());
    }

    #[test]
    fn reads_ribbons_and_interpolates_height() {
        let text = format!(
            r#"<cRecordSet {D}><Record><Network-cTrackNetwork>
            <RibbonContainer><Network-cRibbonContainerUnstreamed><Ribbon>
              <Network-cTrackRibbon>
                <_length d:type="sFloat32">200</_length>
                <RibbonID><cGUID><DevString d:type="cDeltaString">ribbon-a</DevString></cGUID></RibbonID>
                <Height>
                  <Network-iRibbon-cHeight>
                    <_position d:type="sFloat32">200</_position>
                    <_height d:type="sFloat32">30</_height>
                    <_manual d:type="bool">0</_manual>
                  </Network-iRibbon-cHeight>
                  <Network-iRibbon-cHeight>
                    <_position d:type="sFloat32">0</_position>
                    <_height d:type="sFloat32">20</_height>
                    <_manual d:type="bool">1</_manual>
                  </Network-iRibbon-cHeight>
                </Height>
                <Properties><Network-cPropertyContainer><Property>
                  <Network-cTrackNetworkSpeedLimit>
                    <_start d:type="sFloat32">0</_start><_end d:type="sFloat32">200</_end>
                  </Network-cTrackNetworkSpeedLimit>
                  <Network-cTrackMarkerProperty>
                    <Entity><cOwnedEntity>
                      <Component><Network-cTrackMarkerComponent>
                        <DisplayName d:type="cDeltaString">Nyborg Spor 1</DisplayName>
                      </Network-cTrackMarkerComponent></Component>
                      <BlueprintID><iBlueprintLibrary-cAbsoluteBlueprintID>
                        <BlueprintSetID><iBlueprintLibrary-cBlueprintSetID>
                          <Provider d:type="cDeltaString">Kuju</Provider>
                        </iBlueprintLibrary-cBlueprintSetID></BlueprintSetID>
                        <BlueprintID d:type="cDeltaString">RailNetwork\Markers\Platform marker.xml</BlueprintID>
                      </iBlueprintLibrary-cAbsoluteBlueprintID></BlueprintID>
                    </cOwnedEntity></Entity>
                    <_start d:type="sFloat32">20</_start>
                    <_end d:type="sFloat32">180</_end>
                  </Network-cTrackMarkerProperty>
                </Property></Network-cPropertyContainer></Properties>
              </Network-cTrackRibbon>
            </Ribbon><Node>
              <Network-cTrackNode>
                <Connection>
                  <Network-cNetworkNode-sRConnection>
                    <_id><cGUID><DevString d:type="cDeltaString">ribbon-a</DevString></cGUID></_id>
                    <_end><cNormFloat><Position d:type="sFloat32">1</Position></cNormFloat></_end>
                  </Network-cNetworkNode-sRConnection>
                  <Network-cNetworkNode-sRConnection>
                    <_id><cGUID><DevString d:type="cDeltaString">ribbon-b</DevString></cGUID></_id>
                    <_end><cNormFloat><Position d:type="sFloat32">0</Position></cNormFloat></_end>
                  </Network-cNetworkNode-sRConnection>
                </Connection>
                <PatternRef><Network-cTrackNode-sPatternRef>
                  <Pattern><Network-cTurnoutPattern><Manual d:type="bool">0</Manual></Network-cTurnoutPattern></Pattern>
                </Network-cTrackNode-sPatternRef></PatternRef>
              </Network-cTrackNode>
              <Network-cTrackNode>
                <Connection/>
                <PatternRef><Network-cTrackNode-sPatternRef>
                  <Pattern><d:nil/></Pattern>
                </Network-cTrackNode-sPatternRef></PatternRef>
              </Network-cTrackNode>
            </Node></Network-cRibbonContainerUnstreamed></RibbonContainer>
            </Network-cTrackNetwork></Record></cRecordSet>"#
        );
        let network = parse_track_network(&xml::parse_str(&text).unwrap()).unwrap();
        assert_eq!(network.nodes.len(), 2);
        assert_eq!(
            network.nodes[0].connections,
            [
                Connection {
                    ribbon_id: "ribbon-a".into(),
                    end: RibbonEnd::End
                },
                Connection {
                    ribbon_id: "ribbon-b".into(),
                    end: RibbonEnd::Start
                },
            ]
        );
        assert_eq!(
            network.nodes[0].pattern.as_deref(),
            Some("Network-cTurnoutPattern")
        );
        assert_eq!(
            network.nodes[1],
            Node {
                connections: vec![],
                pattern: None
            }
        );
        let ribbons = &network.ribbons;
        assert_eq!(ribbons.len(), 1);
        let ribbon = &ribbons[0];
        assert_eq!((ribbon.id.as_str(), ribbon.length), ("ribbon-a", 200.0));
        assert!(
            ribbon.heights[0].manual,
            "height points are sorted by position"
        );
        assert_eq!(ribbon.height_at(-5.0), Some(20.0));
        assert_eq!(ribbon.height_at(50.0), Some(22.5));
        assert_eq!(ribbon.height_at(500.0), Some(30.0));
    }
}
