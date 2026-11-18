# Open Train Simulator Classic (opentsc)

An open reimplementation of the Train Simulator Classic engine in Rust, in
the spirit of openOMSI: new code that runs the routes and trains you already
own.

**Status: step 1 of many.** There is no simulator yet. What exists is the
foundation everything else needs: reading the game's files.

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
opentsc scan   "D:\SteamLibrary\steamapps\common\RailWorks"
opentsc bin2xml "...\Content\Routes\<guid>\Networks\Tracks.bin" tracks.xml
opentsc bin2xml "...\Scenery\NakskovSt.GeoPcDx" --elide-blobs
opentsc blueprint "D:\...\RailWorks" Kuju RailSimulator "Scenery\Vegetation\Tree_willow_01_Mid.xml"
opentsc ap list "...\Assets\RSC\UKLEDsignals01\UKLEDsignals01Assets.ap"
```

`scan` is the important one right now: it parses every SERZ file in the
installation, including those inside archives, and lists what fails. Each
failure is a gap in [docs/FORMATS.md](docs/FORMATS.md).

To run the tests that need real files:

```
OPENTSC_RAILWORKS="D:\SteamLibrary\steamapps\common\RailWorks" cargo test
```

(PowerShell: `$env:OPENTSC_RAILWORKS = "D:\..."; cargo test`)

## Layout

```
crates/formats   opentsc-formats: file format readers, no rendering, no game logic
crates/cli       opentsc: the command line tool
docs/FORMATS.md  what is known about the formats, and how sure we are
```

More crates follow the same one-per-subsystem split as the project grows
(scripts, simulation, renderer, sound, launcher).

## Roadmap

1. ~~Read archives and SERZ files~~
2. Route viewer: interpret terrain tiles, track ribbons, scenery placement,
   `GeoPcDx` meshes and `TgPcDx` textures; draw one route with a free camera
   (wgpu).
3. Train on track: track graph, one consist rolling with simple physics.
4. Lua and cab: the engine and signal script API, controls, cab view, sound.
5. Signals, AI traffic, scenarios.
6. Launcher.

## License

GNU Affero General Public License v3.0 only (`AGPL-3.0-only`), see
[LICENSE](LICENSE). This is the same license as openOMSI, so code can move
between the two projects.

Train Simulator Classic and RailWorks are trademarks of their owners. This
project is independent and not affiliated with or endorsed by them.
