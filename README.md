# Open Train Simulator Classic (opentsc)

An open reimplementation of the Train Simulator Classic engine in Rust, in
the spirit of openOMSI: new code that runs the routes and trains you already
own.

**Status: early.** There is no simulator yet. What exists is the foundation
everything else needs: reading the game's files, and knowing exactly where
the track runs.

## Principles

1. **No original code or content.** Nothing from the game is in this
   repository, and the game's executables are not disassembled. File formats
   are documented from the files themselves in [docs/FORMATS.md](docs/FORMATS.md).
2. **Your own installation is required.** opentsc reads a RailWorks folder you
   point it at.
3. **The RailWorks folder is never written to.**

## What works

| | |
|---|---|
| `SERZ` binaries (`.bin`, `.proxybin`, `.GeoPcDx`, `.TgPcDx`, route and scenario files) | read into a tree, export to XML |
| XML twins (`RouteProperties.xml`, ...) | read into the same tree |
| `.ap` archives | list, read |
| `Blueprints.pak` | list, read |
| Installation | list routes; resolve *(provider, product, path)* to a file, loose or archived |
| Track geometry | straights, arcs and easements of a route, joined into ribbons, to millimetre accuracy |
| Track network | nodes that connect ribbon ends: joints, turnouts, crossings |
| Named places | platforms, sidings and destinations along the track |
| Paths | the shortest way a train can take between two places, through turnouts the right way |
| Terrain | height tiles |
| Map | draw a route's track and terrain from above as SVG |

Models and textures are readable as trees (materials, vertex declarations,
buffers and mip levels as raw bytes) but not yet interpreted.

## Build

Needs a current stable Rust toolchain (<https://rustup.rs>).

```
cargo build --release
cargo test
```

The binary is `target/release/opentsc` (`opentsc.exe` on Windows).

## Try it

```
opentsc routes "D:\SteamLibrary\steamapps\common\RailWorks"
opentsc map    "D:\SteamLibrary\steamapps\common\RailWorks" "Munich" munich.svg
opentsc markers "D:\SteamLibrary\steamapps\common\RailWorks" "Jylland" "Aarhus H"
opentsc path   "D:\SteamLibrary\steamapps\common\RailWorks" "Jylland" "Aarhus H" "Frederikshavn" path.svg
opentsc scan   "D:\SteamLibrary\steamapps\common\RailWorks"
opentsc bin2xml "...\Content\Routes\<guid>\Networks\Tracks.bin" tracks.xml
opentsc bin2xml "...\Scenery\NakskovSt.GeoPcDx" --elide-blobs
opentsc blueprint "D:\...\RailWorks" Kuju RailSimulator "Scenery\Vegetation\Tree_willow_01_Mid.xml"
opentsc ap list "...\Assets\RSC\UKLEDsignals01\UKLEDsignals01Assets.ap"
```

`map` draws a route and prints how well its curves fit together; open the
SVG in a browser. `path` prints the distance and the stations passed, and
draws the way on the map. `scan` parses every SERZ file in the installation,
including those inside archives, and lists what fails. Each failure is a gap
in [docs/FORMATS.md](docs/FORMATS.md).

To run the tests that need real files:

```
OPENTSC_RAILWORKS="D:\SteamLibrary\steamapps\common\RailWorks" cargo test
```

(PowerShell: `$env:OPENTSC_RAILWORKS = "D:\..."; cargo test`)

## Layout

```
crates/formats   opentsc-formats: file format readers, no rendering, no game logic
crates/route     opentsc-route: track geometry, terrain, route maps
crates/cli       opentsc: the command line tool
docs/FORMATS.md  what is known about the formats, and how sure we are
docs/JOINING.md  joining two routes into one network: the Storebaelt example
```

More crates follow the same one-per-subsystem split as the project grows
(scripts, simulation, renderer, sound, launcher).

## Roadmap

1. ~~Read archives and SERZ files~~
2. Route viewer
   - ~~track geometry and terrain heights, checked with a 2D map~~
   - ~~nodes, so ribbons form a connected network~~
   - ~~named places and paths between them~~
   - junction patterns: turnout geometry and state
   - `GeoPcDx` meshes and `TgPcDx` textures
   - scenery placement, track cross sections (lofts)
   - draw one route in 3D with a free camera (wgpu)
3. Train on track: one consist rolling with simple physics along a path.
4. Lua and cab: the engine and signal script API, controls, cab view, sound.
5. Signals, AI traffic, scenarios.
6. Joined routes: several routes loaded as one network, met at a seam
   (see [docs/JOINING.md](docs/JOINING.md)).
7. Launcher.

## License

GNU Affero General Public License v3.0 only (`AGPL-3.0-only`), see
[LICENSE](LICENSE). This is the same license as openOMSI, so code can move
between the two projects.

Train Simulator Classic and RailWorks are trademarks of their owners. This
project is independent and not affiliated with or endorsed by them.
