//! Track curve geometry.
//!
//! Track centre lines are chains of three kinds of curve: straights, circular
//! arcs and easements (transition spirals between a straight and an arc).
//! All of them live in the horizontal plane; height is handled separately.
//!
//! Coordinates are route metres in `f64`: `x` grows to the east, `z` to the
//! north. A heading is the angle of the direction of travel, measured
//! counter-clockwise from +x, so a positive curvature turns left.
//!
//! The formulas here were fitted to real routes: with them the end of each
//! curve meets the start of the next to within millimetres. See
//! `docs/FORMATS.md` for the evidence and the open questions.

/// A point or direction in the horizontal plane: `[x, z]`.
pub type Vec2 = [f64; 2];

/// Position and direction of travel at one point of a curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub position: Vec2,
    /// Radians, counter-clockwise from +x.
    pub heading: f64,
}

impl Pose {
    /// Unit vector in the direction of travel.
    pub fn tangent(&self) -> Vec2 {
        [self.heading.cos(), self.heading.sin()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CurveKind {
    Straight,
    Arc {
        /// Signed curvature in 1/m, positive turning left.
        curvature: f64,
    },
    Easement(Easement),
}

/// Parameters of an easement, as stored in a track tile.
///
/// The curve is a piece of a *reference spiral*, optionally shifted sideways
/// by `offset` (the second track of a double line is stored as an offset
/// copy of the first track's spiral).
///
/// The spiral is not an exact clothoid. In its own frame, with `z` along the
/// initial direction and `x` to the right of it, the point at parameter `p`
/// is the clothoid's series cut off after the first terms:
///
/// ```text
/// x = curve_sign * A p³ / 6        z = p - A² p⁵ / 40        (A = sharpness)
/// ```
///
/// and its direction is the derivative of that, `(curve_sign * A p² / 2,
/// 1 - A² p⁴ / 8)`. The game stores exactly these values in each easement
/// (`L0Offset`, `L0Tangent`), which is how the formula was found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Easement {
    /// How fast the curvature grows along the spiral (1/m²).
    pub sharpness: f64,
    /// Sideways shift from the reference spiral, in metres. For a curve
    /// that runs forwards (`traversal_sign` +1) a positive offset lies on the
    /// inside of the bend; for one that runs backwards, on the outside.
    pub offset: f64,
    /// Where on the spiral the curve starts, in the file's own measure
    /// (see [`Easement::parameter`]).
    pub l0: f64,
    /// +1 if the spiral bends to the right, -1 if to the left, when followed
    /// in the direction of growing parameter.
    pub curve_sign: f64,
    /// +1 if the curve runs towards growing parameter (it tightens), -1 if
    /// it runs back towards the spiral's origin (it straightens out).
    pub traversal_sign: f64,
}

impl Easement {
    /// Spiral parameter at stored distance `u`.
    ///
    /// The file measures positions on an offset spiral as
    /// `u = p - offset * A * p² / 2`; this inverts that.
    fn parameter(&self, u: f64) -> f64 {
        let c = -self.offset * self.sharpness / 2.0;
        if c.abs() < 1e-15 {
            return u;
        }
        let discriminant = (1.0 + 4.0 * c * u).max(0.0);
        (discriminant.sqrt() - 1.0) / (2.0 * c)
    }

    /// Point and forward unit direction at parameter `p`, in the spiral's
    /// own frame, offset included.
    fn local(&self, p: f64) -> (Vec2, Vec2) {
        let a = self.sharpness;
        let x = self.curve_sign * a * p.powi(3) / 6.0;
        let z = p - a * a * p.powi(5) / 40.0;
        let (tx, tz) = (
            self.curve_sign * a * p * p / 2.0,
            1.0 - a * a * p.powi(4) / 8.0,
        );
        let norm = tx.hypot(tz);
        let (tx, tz) = (tx / norm, tz / norm);
        // (tz, -tx) points to the right of the forward direction.
        let side = self.offset * self.curve_sign * self.traversal_sign;
        ([x + side * tz, z - side * tx], [tx, tz])
    }

    /// Pose at stored distance `s` along a curve that starts at `start`
    /// heading along `tangent`.
    fn pose(&self, start: Vec2, tangent: Vec2, s: f64) -> Pose {
        let (from, from_dir) = self.local(self.parameter(self.l0));
        let (to, to_dir) = self.local(self.parameter(self.l0 + self.traversal_sign * s));
        // Rotation that turns the local direction of travel at the start
        // into the stored start tangent.
        let travel = [
            self.traversal_sign * from_dir[0],
            self.traversal_sign * from_dir[1],
        ];
        let cos = travel[0] * tangent[0] + travel[1] * tangent[1];
        let sin = travel[0] * tangent[1] - travel[1] * tangent[0];
        let rotate = |v: Vec2| [cos * v[0] - sin * v[1], sin * v[0] + cos * v[1]];
        let moved = rotate([to[0] - from[0], to[1] - from[1]]);
        let direction = rotate([
            self.traversal_sign * to_dir[0],
            self.traversal_sign * to_dir[1],
        ]);
        Pose {
            position: [start[0] + moved[0], start[1] + moved[1]],
            heading: direction[1].atan2(direction[0]),
        }
    }

    /// Largest curvature reached on a piece of stored length `length`.
    fn peak_curvature(&self, length: f64) -> f64 {
        let a = self.parameter(self.l0).abs();
        let b = self.parameter(self.l0 + self.traversal_sign * length).abs();
        self.sharpness.abs() * a.max(b)
    }
}

/// One piece of track centre line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curve {
    pub start: Vec2,
    /// Unit direction of travel at the start.
    pub tangent: Vec2,
    /// Length as stored, in metres. Distances along a ribbon (signal
    /// positions, height points) count in these units. For an offset
    /// easement that runs backwards this is not quite the geometric length.
    pub length: f64,
    pub kind: CurveKind,
}

impl Curve {
    /// Pose at distance `s` from the start, `0 <= s <= length`.
    pub fn pose_at(&self, s: f64) -> Pose {
        let s = s.clamp(0.0, self.length.max(0.0));
        let heading = self.tangent[1].atan2(self.tangent[0]);
        let along_tangent = Pose {
            position: [
                self.start[0] + self.tangent[0] * s,
                self.start[1] + self.tangent[1] * s,
            ],
            heading,
        };
        match self.kind {
            CurveKind::Straight => along_tangent,
            CurveKind::Arc { curvature } if curvature.abs() < 1e-12 => along_tangent,
            CurveKind::Arc { curvature } => {
                let end = heading + curvature * s;
                Pose {
                    position: [
                        self.start[0] + (end.sin() - heading.sin()) / curvature,
                        self.start[1] - (end.cos() - heading.cos()) / curvature,
                    ],
                    heading: end,
                }
            }
            CurveKind::Easement(e) => e.pose(self.start, self.tangent, s),
        }
    }

    /// Pose at the end of the curve.
    pub fn end(&self) -> Pose {
        self.pose_at(self.length)
    }

    /// Points along the curve, start and end included, close enough together
    /// that straight lines between them stay within `tolerance` metres of it.
    pub fn polyline(&self, tolerance: f64) -> Vec<Vec2> {
        let tolerance = tolerance.max(1e-3);
        let peak_curvature = match self.kind {
            CurveKind::Straight => 0.0,
            CurveKind::Arc { curvature } => curvature.abs(),
            CurveKind::Easement(e) => e.peak_curvature(self.length),
        };
        // A chord of length c on a circle of curvature k sags by k c² / 8.
        let segments = if peak_curvature < 1e-9 {
            1
        } else {
            let chord = (8.0 * tolerance / peak_curvature).sqrt();
            ((self.length / chord).ceil() as usize).clamp(1, 4096)
        };
        (0..=segments)
            .map(|i| {
                self.pose_at(self.length * i as f64 / segments as f64)
                    .position
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI};

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn straight_runs_along_its_tangent() {
        let c = Curve {
            start: [10.0, 20.0],
            tangent: [0.6, 0.8],
            length: 50.0,
            kind: CurveKind::Straight,
        };
        let end = c.end();
        assert!(close(end.position[0], 40.0, 1e-9) && close(end.position[1], 60.0, 1e-9));
        assert_eq!(c.polyline(0.1).len(), 2);
    }

    #[test]
    fn quarter_circle_to_the_left() {
        // Heading east, radius 100, turning left for a quarter turn ends
        // heading north at (100, 100).
        let c = Curve {
            start: [0.0, 0.0],
            tangent: [1.0, 0.0],
            length: 100.0 * FRAC_PI_2,
            kind: CurveKind::Arc { curvature: 0.01 },
        };
        let end = c.end();
        assert!(close(end.position[0], 100.0, 1e-9));
        assert!(close(end.position[1], 100.0, 1e-9));
        assert!(close(end.heading, FRAC_PI_2, 1e-12));
        // Every sampled point is on the circle around (0, 100).
        let points = c.polyline(0.01);
        for p in &points {
            assert!(close(
                (p[0].powi(2) + (p[1] - 100.0).powi(2)).sqrt(),
                100.0,
                1e-9
            ));
        }
        // And chords stay within the requested tolerance of the arc.
        let chord = (points[1][0] - points[0][0]).hypot(points[1][1] - points[0][1]);
        assert!(0.01 * chord * chord / 8.0 <= 0.0101);
    }

    fn spiral(length: f64, sharpness: f64, curve_sign: f64) -> Curve {
        Curve {
            start: [0.0, 0.0],
            tangent: [0.0, 1.0],
            length,
            kind: CurveKind::Easement(Easement {
                sharpness,
                offset: 0.0,
                l0: 0.0,
                curve_sign,
                traversal_sign: 1.0,
            }),
        }
    }

    #[test]
    fn easement_follows_the_truncated_series() {
        // Heading north and bending right, so the spiral's own frame and the
        // route frame coincide: x = A L³/6, z = L - A² L⁵/40.
        let (length, sharpness) = (300.0, 8.0e-6);
        let end = spiral(length, sharpness, 1.0).end();
        let theta = sharpness * length * length / 2.0;
        assert!(close(
            end.position[0],
            sharpness * length.powi(3) / 6.0,
            1e-9
        ));
        assert!(close(
            end.position[1],
            length - sharpness.powi(2) * length.powi(5) / 40.0,
            1e-9
        ));
        assert!(close(
            end.heading,
            (1.0 - theta * theta / 2.0).atan2(theta),
            1e-12
        ));
        // A gentle spiral is indistinguishable from a clothoid, whose heading
        // turns by A L²/2.
        let gentle = spiral(40.0, 3.35e-5, 1.0).end();
        assert!(close(
            gentle.heading,
            FRAC_PI_2 - 3.35e-5 * 40.0 * 40.0 / 2.0,
            1e-5
        ));
        // The mirror image bends left.
        let left = spiral(length, sharpness, -1.0).end();
        assert!(close(left.position[0], -end.position[0], 1e-12));
        assert!(close(left.heading, PI - end.heading, 1e-12));
    }

    #[test]
    fn easement_backwards_retraces_the_forward_one() {
        // Run a spiral out, then run the reverse piece from its end pose:
        // it must arrive back at the origin heading the opposite way.
        let (length, sharpness) = (60.0, 6.5e-5);
        let end = spiral(length, sharpness, 1.0).end();
        let back = Curve {
            start: end.position,
            tangent: [-end.heading.cos(), -end.heading.sin()],
            length,
            kind: CurveKind::Easement(Easement {
                sharpness,
                offset: 0.0,
                l0: length,
                curve_sign: 1.0,
                traversal_sign: -1.0,
            }),
        };
        let home = back.end();
        assert!(close(home.position[0], 0.0, 1e-9) && close(home.position[1], 0.0, 1e-9));
        assert!(close(
            (home.heading - (FRAC_PI_2 + PI)).rem_euclid(2.0 * PI),
            0.0,
            1e-9
        ));
    }

    #[test]
    fn offset_easement_stays_parallel_to_its_reference() {
        // An offset spiral started 4 m beside the reference, with the same
        // heading, stays 4 m away from the matching reference point and
        // keeps the same heading there. Travelling forwards, a positive
        // offset is on the inside of the bend: here to the east, since the
        // spiral heads north and bends right.
        let (sharpness, offset, length) = (3.35e-5, 4.0, 80.0);
        let side = Curve {
            start: [offset, 0.0],
            tangent: [0.0, 1.0],
            length,
            kind: CurveKind::Easement(Easement {
                sharpness,
                offset,
                l0: 0.0,
                curve_sign: 1.0,
                traversal_sign: 1.0,
            }),
        };
        let end = side.end();
        // Reference parameter reached: solve u = p - offset A p²/2 for u = length.
        let c = -offset * sharpness / 2.0;
        let p = ((1.0 + 4.0 * c * length).sqrt() - 1.0) / (2.0 * c);
        assert!(p > length, "the inside track is shorter than its reference");
        let reference = spiral(p, sharpness, 1.0).end();
        assert!(close(end.heading, reference.heading, 1e-9));
        let gap = (end.position[0] - reference.position[0])
            .hypot(end.position[1] - reference.position[1]);
        assert!(close(gap, offset, 1e-9), "gap {gap}");
    }

    #[test]
    fn polyline_of_an_easement_ends_at_its_end_pose() {
        let c = spiral(75.0, 1.2e-4, -1.0);
        let points = c.polyline(0.02);
        let end = c.end();
        let last = points[points.len() - 1];
        assert!(points.len() > 3);
        assert!(close(last[0], end.position[0], 1e-9) && close(last[1], end.position[1], 1e-9));
    }
}
