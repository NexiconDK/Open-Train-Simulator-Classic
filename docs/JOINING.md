# Joining routes

Routes are separate worlds: each has its own origin, its own tiles and its
own track network. Two routes that model neighbouring parts of the same
railway can still be driven as one, if they are joined at a **seam**.

Nothing here is implemented in the engine yet. This note records the method
and the first worked example, so that the loader can be built to it.

## Method

A seam is a pair of places, one on each route, that stand for the same spot
on the real line. It works when, at that spot, both routes have

- the same number of tracks at the same spacing,
- the same gradient (level is easiest),
- curvature close enough that the step cannot be felt.

One route keeps its coordinates. The other is moved as a rigid body: turned
about the vertical axis, shifted, and raised or lowered, so that position,
direction and height of the track agree at the seam. Each route is then used
on its own side of the seam only; what it has beyond the seam is left out.

Because each route was laid out by hand, two routes covering the same
stretch do not lie on top of each other anywhere else. The seam does not try
to make them. It only has to be exact at one cross-section of the line.

## Example: Jylland and Sjaelland across Storebaelt

Both routes contain the whole Great Belt crossing with a station at each
end. Jylland's track starts 2.2 km east of Korsoer; Sjaelland's ends 2.2 km
west of Nyborg. So about 28 km of railway exists twice.

The two versions were clearly built from the same real line, but separately:

| | Sjaelland | Jylland |
|---|---|---|
| Korsoer platform 1 to Nyborg platform 1 | 22.948 km | 24.695 km |
| tunnel, modelled as a level stretch at | -10.85 m | -11.67 m |
| bridge deck, level at | 8.40 m | 9.38 m |
| length of the level deck | 6 939 m | 7 164 m |
| tracks on the deck | 2, 4.200 m apart | 2, 4.200 m apart |
| curve off the bridge towards Nyborg | 47.0 degrees, R about 1 590 m | 47.5 degrees, R about 1 740 m |
| straight between that curve and Nyborg | 1 150 m | 1 145 m |

Sjaelland's coordinates are true to the map (origin at Koebenhavn H).
Jylland's are not tied to the map and are turned about 15 degrees against
Sjaelland's here.

### The seam

On the level straight between Korsoer station and the tunnel ramp, 0.9 km
west of the platforms. Sjaelland is used up to and including Korsoer
station; from there on the tunnel, Sprogoe, the bridge and Nyborg are
Jylland's. Both routes have plain double track here, level, between the
last turnout of the station and the start of the ramp.

| | Sjaelland | Jylland |
|---|---|---|
| ribbon | `9ee1f4d3-bc3a-434a-9308-9b8a6fa18c9d` at 111.0 m | `9039aea0-47c8-43a3-add6-2b42554ae0af` at 118.0 m |
| position (x east, z north) | -92 443.67, -30 998.59 | 106 522.41, -279 488.18 |
| direction of travel westwards | 200.105 degrees | 229.378 degrees |
| height | 0.364 m | 1.360 m |
| other track | `6b8e3d6c-7e33-4b90-b69e-07e6f3f3c8a3` at 31.2 m | `09e48540-c880-40e5-88de-ddb810eb6e61` at 40.2 m |

Directions are measured anticlockwise from east.

To bring Jylland into Sjaelland's frame: turn it by -29.273 degrees about
the seam point, move the seam point onto Sjaelland's, and lower everything
by 0.996 m.

- The first track then agrees by construction. The second track lands
  0.6 mm from Sjaelland's, at the same height.
- Both tracks are straight at the seam.
- With this placement Jylland's Korsoer platforms lie 80 m from
  Sjaelland's, so the two stations are nearly the same shape. Jylland's
  Nyborg lies 4.7 km from where Sjaelland has its Nyborg: the two crossings
  are laid out differently, and only one of them can be kept.
- The seam is on land. Terrain and scenery of the two routes meet here and
  will need attention.

### The trip

| | |
|---|---|
| Koebenhavn H to the seam (via Koege Nord, Ringsted, Korsoer) | 106.723 km |
| seam to Aarhus H (Storebaelt, Nyborg, Odense, Fredericia, Vejle, Horsens) | 224.335 km |
| Aarhus H to Frederikshavn (Randers, Hobro, Aalborg, Hjoerring) | 217.983 km |
| **Koebenhavn H to Frederikshavn** | **549.04 km** |

Aarhus H is entered and left at the same end, as in reality, so the train
changes direction there.

### Other possible seams

- The middle of the level deck of the west bridge: Sjaelland
  `06cbbba5-2fb6-4433-a6e8-d210e9a25e94` at 30.1 m and Jylland
  `8297e759-7713-46a1-a2a5-a2b224e45cbf` at 545.0 m; turn Jylland by
  -15.437 degrees and lower it by 0.988 m; second track within 0.8 mm.
  Water on both sides, so only the bridge itself has to meet. Sjaelland
  keeps Korsoer, the tunnel and Sprogoe; Jylland keeps Nyborg. The trip is
  then 547.46 km.
- East of Korsoer station, where both routes have a level double-track
  straight. Jylland's Korsoer would be kept as well.
- The level straight east of Nyborg station, which is 1 150 m long in both
  routes. Sjaelland's whole crossing would lead into Jylland's Nyborg.
- Sjaelland's west end, on a 495 m straight. Sjaelland's Nyborg would be
  kept and Jylland's left out.

## What a loader has to do

- Read a small description of the join: the two routes, the seam place on
  each track of each route, and which side of the seam each route keeps.
- Cut the ribbons at the seam and connect them with new nodes, giving one
  track graph.
- Load tiles of both routes, with the moved route's tiles, scenery and
  terrain turned and shifted, and skip everything beyond the seam.
- Decide what to do where the two routes' scenery meets. Over water this is
  only the bridge itself.

## How this was worked out

With `opentsc markers`, `opentsc path` and `opentsc track-points` on both
routes: the platforms of Korsoer and Nyborg were found in each, the main
line between them was followed in each, and height, direction and track
spacing were compared along it.
