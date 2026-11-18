//! Geometry checks against real routes.
//!
//! Only runs when `OPENTSC_RAILWORKS` points at an installation; see the
//! README. For every route it reads all track tiles and checks that the
//! curves fit together, which is the strongest evidence that the curve
//! formulas match the game's.

use std::path::PathBuf;

use opentsc_formats::Install;
use opentsc_route::map::{NodeCheck, TrackMap};
use opentsc_route::Route;

#[test]
fn curves_of_every_route_fit_together() {
    let Some(root) = std::env::var_os("OPENTSC_RAILWORKS").map(PathBuf::from) else {
        eprintln!("OPENTSC_RAILWORKS is not set; skipping");
        return;
    };
    let install = Install::open(root).expect("open installation");
    let mut checked = 0;
    for info in install.routes().expect("list routes") {
        let route = Route::open(&info.path).expect("open route");
        let name = route.name.clone().unwrap_or_else(|| route.id.clone());
        let map = TrackMap::build(&route, 0.5).unwrap_or_else(|e| panic!("{name}: {e}"));
        if map.curves() == 0 {
            continue;
        }
        checked += 1;
        assert!(
            map.unknown_curves.is_empty(),
            "{name}: unknown curves {:?}",
            map.unknown_curves
        );
        assert!(
            map.broken_ribbons.is_empty(),
            "{name}: ribbons with gaps: {:?}",
            &map.broken_ribbons[..map.broken_ribbons.len().min(5)]
        );
        assert!(
            map.gap_quantile(1.0) < 0.05,
            "{name}: widest joint is {:.3} m",
            map.gap_quantile(1.0)
        );

        // Wherever a ribbon has curves, their lengths add up to its length.
        let network = route
            .track_network()
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        for ribbon in &network.ribbons {
            if let Some(sum) = map.ribbon_lengths.get(&ribbon.id) {
                assert!(
                    (sum - ribbon.length).abs() < 0.05,
                    "{name}: ribbon {} is {} m but its curves add up to {sum} m",
                    ribbon.id,
                    ribbon.length
                );
            }
        }

        // Ribbon ends that a node joins are in the same place. A few nodes
        // in hand-built routes are genuinely off (up to 1.8 m seen), so the
        // check is on the 99th percentile: taking the wrong end of a ribbon
        // anywhere in the reader would move far more than 1% of the nodes.
        let nodes = NodeCheck::build(&network, &map);
        assert!(
            nodes.spread_quantile(0.99) < 0.02,
            "{name}: ribbon ends at nodes are {:.3} m apart at the 99th percentile",
            nodes.spread_quantile(0.99)
        );
    }
    eprintln!("checked {checked} routes");
}
