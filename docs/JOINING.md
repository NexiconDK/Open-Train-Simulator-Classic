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

The middle of the level bridge deck, in each route. There is water on both
sides, the track is level and double in both routes, and each route keeps
all of its own land: Sjaelland keeps Korsoer, the tunnel and Sprogoe;
Jylland keeps Nyborg and everything west of it.

| | Sjaelland | Jylland |
|---|---|---|
| ribbon | `06cbbba5-2fb6-4433-a6e8-d210e9a25e94` at 30.1 m | `8297e759-7713-46a1-a2a5-a2b224e45cbf` at 545.0 m |
| position (x east, z north) | -106 589.90, -35 034.54 | 92 712.30, -288 451.59 |
| direction of travel westwards | 202.155 degrees | 217.592 degrees |
| height | 8.397 m | 9.385 m |
| other track | `ce3d53ee-70bb-4595-8fb0-47d74a7018ac` at 530.2 m | `fb9f519a-e35a-4faf-bc45-6a400e8ffc1d` at 545.0 m |

Directions are measured anticlockwise from east.

To bring Jylland into Sjaelland's frame: turn it by -15.437 degrees about
the seam point, move the seam point onto Sjaelland's, and lower everything
by 0.988 m.

- The first track then agrees by construction. The second track lands
  0.8 mm from Sjaelland's, at the same height.
- The curvature steps from a radius of about 20 km to about 57 km, which is
  far below anything a train reacts to.
- The joined deck is 7 051 m long.
- With this placement Jylland's Nyborg lies 0.70 km from where Sjaelland
  has its Nyborg, and Jylland's Korsoer 1.77 km from Sjaelland's. That is
  the measure of how differently the two were laid out, and the reason only
  one version of each station can be kept.

### The trip

| | |
|---|---|
| Koebenhavn H to the seam (via Koege Nord, Ringsted, Korsoer) | 121.747 km |
| seam to Aarhus H (Nyborg, Odense, Fredericia, Vejle, Horsens, Skanderborg) | 207.730 km |
| Aarhus H to Frederikshavn (Randers, Hobro, Aalborg, Hjoerring) | 217.983 km |
| **Koebenhavn H to Frederikshavn** | **547.46 km** |

Aarhus H is entered and left at the same end, as in reality, so the train
changes direction there.

### Other possible seams

- The level straight east of Nyborg station, which is 1 150 m long in both
  routes. Jylland's Storebaelt would then go unused and Sjaelland's bridge
  would lead into Jylland's Nyborg.
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
