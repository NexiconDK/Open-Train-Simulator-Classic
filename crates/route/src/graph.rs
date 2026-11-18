//! The track network as a graph a train can be routed through.
//!
//! Ribbons are the edges and nodes are where they meet, but a train cannot
//! take every pair of ribbons at a node: at a turnout it can go from the
//! single leg to either of the two diverging legs, never from one diverging
//! leg to the other. So the search runs over *(ribbon, direction of travel)*
//! and only follows a link when the track carries straight on through the
//! node.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::geom::Pose;
use crate::track::{RibbonEnd, TrackNetwork};

/// A train may only pass from one ribbon to the next if their directions at
/// the node differ by less than this. Real turnouts diverge by a few degrees;
/// the wrong pair of legs differs by nearly 180.
const MAX_TURN: f64 = 0.6; // radians, about 34 degrees

/// Where a ribbon starts and ends and how long it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RibbonShape {
    /// Pose at distance 0, heading towards growing distance.
    pub start: Pose,
    /// Pose at the far end, same sense of heading.
    pub end: Pose,
    pub length: f64,
}

/// A position on the network: a ribbon and a distance along it.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub ribbon_id: String,
    pub distance: f64,
}

/// Direction of travel along a ribbon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Towards growing distance.
    Forward,
    Backward,
}

/// One step of a path: a ribbon and which way it is travelled.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub ribbon_id: String,
    pub direction: Direction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    /// Distance travelled in metres.
    pub length: f64,
    /// Ribbons in order of travel, first and last included.
    pub steps: Vec<Step>,
}

pub struct TrackGraph {
    ids: Vec<String>,
    index: HashMap<String, usize>,
    lengths: Vec<f64>,
    /// For each state (ribbon * 2 + direction): the states reachable after
    /// leaving the ribbon at its far end.
    links: Vec<Vec<usize>>,
}

impl TrackGraph {
    /// Build the graph from the nodes of `network` and the shape of each
    /// ribbon. Ribbons without a shape are left out.
    pub fn build(network: &TrackNetwork, shapes: &HashMap<String, RibbonShape>) -> Self {
        let mut ids = Vec::new();
        let mut index = HashMap::new();
        let mut lengths = Vec::new();
        let mut poses = Vec::new();
        for ribbon in &network.ribbons {
            if let Some(shape) = shapes.get(&ribbon.id) {
                index.insert(ribbon.id.clone(), ids.len());
                ids.push(ribbon.id.clone());
                lengths.push(ribbon.length);
                poses.push(*shape);
            }
        }

        let mut links = vec![Vec::new(); ids.len() * 2];
        for node in &network.nodes {
            // Each connection, as (ribbon, end at this node).
            let ends: Vec<(usize, RibbonEnd)> = node
                .connections
                .iter()
                .filter_map(|c| Some((*index.get(&c.ribbon_id)?, c.end)))
                .collect();
            for &(from, from_end) in &ends {
                // Arriving at the node along `from`.
                let (arriving_state, arriving_heading) = match from_end {
                    RibbonEnd::End => (from * 2, poses[from].end.heading),
                    RibbonEnd::Start => (
                        from * 2 + 1,
                        poses[from].start.heading + std::f64::consts::PI,
                    ),
                };
                for &(to, to_end) in &ends {
                    if to == from && to_end == from_end {
                        continue;
                    }
                    // Leaving the node along `to`.
                    let (leaving_state, leaving_heading) = match to_end {
                        RibbonEnd::Start => (to * 2, poses[to].start.heading),
                        RibbonEnd::End => {
                            (to * 2 + 1, poses[to].end.heading + std::f64::consts::PI)
                        }
                    };
                    if angle_between(arriving_heading, leaving_heading) < MAX_TURN {
                        links[arriving_state].push(leaving_state);
                    }
                }
            }
        }
        TrackGraph {
            ids,
            index,
            lengths,
            links,
        }
    }

    /// Number of ribbons in the graph.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Where a train can go on after running off the end of `ribbon_id` in
    /// `direction`: each ribbon it can enter, and the direction it then
    /// travels along it.
    pub fn onward(&self, ribbon_id: &str, direction: Direction) -> Vec<Step> {
        let Some(&ribbon) = self.index.get(ribbon_id) else {
            return Vec::new();
        };
        let state = ribbon * 2 + usize::from(direction == Direction::Backward);
        self.links[state]
            .iter()
            .map(|&next| Step {
                ribbon_id: self.ids[next / 2].clone(),
                direction: if next % 2 == 0 {
                    Direction::Forward
                } else {
                    Direction::Backward
                },
            })
            .collect()
    }

    /// Shortest way for a train from `from` to `to`, if there is one.
    /// Reversing on the way is not considered.
    pub fn shortest_path(&self, from: &Place, to: &Place) -> Option<Path> {
        let start = *self.index.get(&from.ribbon_id)?;
        let goal = *self.index.get(&to.ribbon_id)?;
        let states = self.links.len();
        // dist[state]: metres travelled on reaching the far end of the
        // ribbon in that direction.
        let mut dist = vec![f64::INFINITY; states];
        let mut previous = vec![usize::MAX; states];
        let mut queue = BinaryHeap::new();
        let from_distance = from.distance.clamp(0.0, self.lengths[start]);
        let to_distance = to.distance.clamp(0.0, self.lengths[goal]);
        for (state, cost) in [
            (start * 2, self.lengths[start] - from_distance),
            (start * 2 + 1, from_distance),
        ] {
            dist[state] = cost;
            queue.push(Queued { cost, state });
        }
        while let Some(Queued { cost, state }) = queue.pop() {
            if cost > dist[state] {
                continue;
            }
            for &next in &self.links[state] {
                let reached = cost + self.lengths[next / 2];
                if reached < dist[next] {
                    dist[next] = reached;
                    previous[next] = state;
                    queue.push(Queued {
                        cost: reached,
                        state: next,
                    });
                }
            }
        }

        // Stop part-way along the goal ribbon instead of at its far end.
        let mut best: Option<(f64, usize)> = None;
        for (state, overshoot) in [
            (goal * 2, self.lengths[goal] - to_distance),
            (goal * 2 + 1, to_distance),
        ] {
            let entered_elsewhere = previous[state] != usize::MAX;
            // On the start ribbon itself, only the direction that actually
            // passes the goal counts.
            let passes_goal = match state % 2 {
                0 => to_distance >= from_distance,
                _ => to_distance <= from_distance,
            };
            if dist[state].is_finite() && (entered_elsewhere || (goal == start && passes_goal)) {
                let total = dist[state] - overshoot;
                if best.is_none_or(|(length, _)| total < length) {
                    best = Some((total, state));
                }
            }
        }
        let (length, mut state) = best?;
        let mut steps = Vec::new();
        loop {
            steps.push(Step {
                ribbon_id: self.ids[state / 2].clone(),
                direction: if state % 2 == 0 {
                    Direction::Forward
                } else {
                    Direction::Backward
                },
            });
            match previous[state] {
                usize::MAX => break,
                earlier => state = earlier,
            }
        }
        steps.reverse();
        Some(Path { length, steps })
    }
}

/// Smallest angle between two headings, in radians.
fn angle_between(a: f64, b: f64) -> f64 {
    let turn = std::f64::consts::TAU;
    let difference = (a - b).rem_euclid(turn);
    difference.min(turn - difference)
}

/// Queue entry ordered so that the smallest cost comes out first.
struct Queued {
    cost: f64,
    state: usize,
}

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Queued {}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| other.state.cmp(&self.state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{Connection, Node, Ribbon};

    fn ribbon(id: &str, length: f64) -> Ribbon {
        Ribbon {
            id: id.into(),
            length,
            heights: Vec::new(),
        }
    }

    fn shape(start: [f64; 2], end: [f64; 2]) -> RibbonShape {
        let heading = (end[1] - start[1]).atan2(end[0] - start[0]);
        RibbonShape {
            start: Pose {
                position: start,
                heading,
            },
            end: Pose {
                position: end,
                heading,
            },
            length: (end[0] - start[0]).hypot(end[1] - start[1]),
        }
    }

    fn node(ends: &[(&str, RibbonEnd)]) -> Node {
        Node {
            connections: ends
                .iter()
                .map(|(id, end)| Connection {
                    ribbon_id: id.to_string(),
                    end: *end,
                })
                .collect(),
            pattern: None,
        }
    }

    /// A turnout at x = 100: `main` arrives from the west, `ahead` carries
    /// straight on and `branch` diverges slightly to the north. `ahead` is
    /// stored backwards (its end is at the turnout).
    fn turnout() -> (TrackNetwork, HashMap<String, RibbonShape>) {
        let network = TrackNetwork {
            ribbons: vec![
                ribbon("main", 100.0),
                ribbon("ahead", 200.0),
                ribbon("branch", 300.0),
            ],
            nodes: vec![node(&[
                ("main", RibbonEnd::End),
                ("ahead", RibbonEnd::End),
                ("branch", RibbonEnd::Start),
            ])],
            markers: Vec::new(),
        };
        let shapes = HashMap::from([
            ("main".to_string(), shape([0.0, 0.0], [100.0, 0.0])),
            ("ahead".to_string(), shape([300.0, 0.0], [100.0, 0.0])),
            ("branch".to_string(), shape([100.0, 0.0], [398.0, 30.0])),
        ]);
        (network, shapes)
    }

    fn place(ribbon: &str, distance: f64) -> Place {
        Place {
            ribbon_id: ribbon.into(),
            distance,
        }
    }

    #[test]
    fn goes_through_a_turnout_onto_either_leg() {
        let (network, shapes) = turnout();
        let graph = TrackGraph::build(&network, &shapes);
        // 60 m left on `main`, then `ahead` backwards from 200 down to 150.
        let path = graph
            .shortest_path(&place("main", 40.0), &place("ahead", 150.0))
            .unwrap();
        assert!((path.length - 110.0).abs() < 1e-9);
        assert_eq!(path.steps.len(), 2);
        assert_eq!(path.steps[1].direction, Direction::Backward);
        let path = graph
            .shortest_path(&place("main", 40.0), &place("branch", 25.0))
            .unwrap();
        assert!((path.length - 85.0).abs() < 1e-9);
        assert_eq!(path.steps[1].direction, Direction::Forward);
        // And back the other way.
        let back = graph
            .shortest_path(&place("branch", 25.0), &place("main", 40.0))
            .unwrap();
        assert!((back.length - 85.0).abs() < 1e-9);
    }

    #[test]
    fn cannot_turn_from_one_diverging_leg_onto_the_other() {
        let (network, shapes) = turnout();
        let graph = TrackGraph::build(&network, &shapes);
        assert_eq!(
            graph.shortest_path(&place("ahead", 150.0), &place("branch", 25.0)),
            None
        );
    }

    #[test]
    fn stays_on_one_ribbon_when_start_and_goal_share_it() {
        let (network, shapes) = turnout();
        let graph = TrackGraph::build(&network, &shapes);
        let path = graph
            .shortest_path(&place("branch", 250.0), &place("branch", 40.0))
            .unwrap();
        assert!((path.length - 210.0).abs() < 1e-9);
        assert_eq!(
            path.steps,
            [Step {
                ribbon_id: "branch".into(),
                direction: Direction::Backward
            }]
        );
        assert_eq!(
            graph.shortest_path(&place("nowhere", 0.0), &place("main", 0.0)),
            None
        );
    }
}
