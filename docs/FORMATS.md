# File formats

Notes on the formats `opentsc-formats` reads. Everything here was worked out
from files in a legally owned installation: by reading hex dumps, and by
comparing binaries with the XML sources that some add-ons install next to
them. No game executable was disassembled or decompiled.

Each claim is marked:

- **Verified**: checked against real files (see "How this was checked").
- **Assumed**: follows the obvious pattern but no sample exercised it yet.
- **Unknown**: we store the bytes and do not know what they mean.

All integers are little-endian.

## Installation layout

```
RailWorks/
  Assets/<Provider>/<Product>/     assets, loose and/or inside *.ap archives
    Blueprints.pak                 cache of the product's blueprints
  Content/Routes/<guid>/
    RouteProperties.xml            name, template, sky/weather blueprints
    Networks/Tracks.bin            the whole track network
    Networks/Track Tiles/*.bin     per-tile track data
    Networks/{Loft,Road} Tiles/    lofts (fences, platforms ...) and roads
    Scenery/±XXXXXX±YYYYYY.bin     placed objects per tile
    Terrain/±XXXXXX±YYYYYY.bin     height data per tile (blob)
    MixMap/±XXXXXX±YYYYYY.bin      terrain texture mix per tile (blob)
    Scenarios/<guid>/              Scenario.bin, ScenarioProperties.xml ...
```

Content points at assets with a triple *(Provider, Product, path)*. The path
uses `\`, is not consistent about letter case, and names the blueprint's
`.xml` source although only the compiled `.bin` is installed.
(**Verified** on scenery tiles referring to `Kuju\RailSimulator`.)

A route folder that contains only `Scenarios` belongs to a route that is not
installed. A route's own files can also sit inside `MainContent.ap` in its
folder, with the same relative paths as an unpacked route. (**Verified** on
two packed routes.)

An asset archive normally sits in the product folder and holds paths relative
to it. It can also sit one level up, in the provider folder, and then its
paths start with the product name. (**Verified**: `Castlerock\CastlerockAssets.ap`
holds `Scenery/...`.)

Loose files take priority over files inside `.ap` archives. (**Assumed**:
this is the commonly described behaviour; not yet tested against the game.)

## SERZ

A compact binary encoding of an XML-like tree. Seen in `.bin`, `.proxybin`,
`.GeoPcDx` (models), `.TgPcDx` (textures), `.XSec` (loft cross sections) and
all route and scenario `.bin` files.

Not SERZ, and not read yet: `.ban` (animations), `.out` and `.luac` (compiled
Lua), `.lua`, `.dav` (audio), `.dcsv`, `.lan` (language tables), `.sbn`.

### Header

| Offset | Size | Content |
|-------:|-----:|---------|
| 0 | 4 | `SERZ` |
| 4 | 4 | `00 00 01 00` in every sample. Treated as a version. **Unknown** |

### Lines

The rest of the file is a sequence of *lines*. A line starts in one of two
ways:

- `FF <command>` followed by a **definition** and then the line's **data**.
  The definition is stored in the next slot of a ring of 255 slots
  (slot = number of definitions so far, modulo 255). Every command takes a
  slot, including `43` and `42`.
- One byte `00`..`FE`: reuse the definition in that slot, followed only by
  new **data**.

(**Verified**, including the wrap-around: `Tracks.bin` and `Scenario.bin`
samples contain 288 and 378 definitions.)

| Command | Meaning | Definition | Data |
|--------:|---------|------------|------|
| `50` `P` | open element | name: string | id: u32, child count: u32 |
| `70` `p` | close element | name: string | none |
| `56` `V` | value | name: string, type: string | one value of that type |
| `41` `A` | array | name: string, element type: string | count: u8, then that many values |
| `52` `R` | reference | name: string | target id: u32 |
| `4E` `N` | nil | none | none |
| `42` `B` | blob | none | size: u32, then that many bytes |
| `43` `C` | hint | none | flag: u8, value: u32 |

Notes:

- The element **id** is 0 or a number that looks like a memory address at
  save time. A reference's target is the id of some element in the same
  file; 0 is a null reference. (**Verified**: `d:id` and `d:type="ref"` in
  the XML sources match.)
- The **child count** is not reliable: `InitialSave.bin` opens its root with
  a count of 0 and then nests the whole file inside. The reader therefore
  builds the tree from open/close pairs. (**Verified**)
- The **name in a close line is not reliable** either: three engine
  blueprints of one add-on open `iAnimObjectRenderBlueprint::cAnimation` and
  close it as `...::cAutoAnimation`. The reader keeps the opening name and
  counts the mismatch in its report. (**Verified** that such files exist and
  are otherwise well formed; that the game accepts them is **assumed** from
  their presence in the product's `Blueprints.pak`.)
- A value's **name can be empty**; the XML form calls such elements `e`.
  (**Verified**: GUID halves.)
- Command `43` appears once near the start of route, tile, scenario, model
  and texture files, and never in blueprints. Flag is always 0. The value is
  4 for an empty record set and blob size + 4 for terrain and mix-map tiles;
  it looks like a memory-size hint for the loader. **Unknown**
- Replaying a blob line reads a new size and new bytes. (**Verified**:
  texture mip levels and model buffers.) Replaying a hint line is
  **assumed** to read a new flag and value; no sample does it.

### Strings

A string is written as a u16:

- `FFFF`: a new string follows as u32 length + UTF-8 text. It is appended to
  the file's string table.
- anything else: index into the string table.

This applies to names, type names and string values alike; they share one
table. (**Verified**)

The length counts **characters, not bytes**: `Übergabe1` is stored with
length 9 followed by 10 bytes. (**Verified** on German scenario texts.) The
reader decodes UTF-8 character by character and falls back to Latin-1 for
bytes that are not valid UTF-8 (**assumed**: no sample needed the fallback).

### Value types

| Type name | Size | Status |
|-----------|-----:|--------|
| `bool` | 1 | Verified |
| `sUInt8` | 1 | Verified |
| `sInt16`, `sUInt16` | 2 | Verified |
| `sInt32`, `sUInt32` | 4 | Verified |
| `sUInt64` | 8 | Verified |
| `sFloat32` | 4 | Verified |
| `cDeltaString` | string (see above) | Verified |
| `sInt8`, `sInt64`, `sFloat64` | 1, 8, 8 | Assumed, never seen |

Enumerations are stored as `cDeltaString` (`eTrue`, `eShadowType_Blobby`).

### XML form

The same tree as XML, as found in `RouteProperties.xml`,
`ScenarioProperties.xml` and blueprint sources:

```xml
<cRouteProperties xmlns:d="http://www.kuju.com/TnT/2003/Delta" d:version="1.0" d:id="134209024">
  <Name d:type="cDeltaString">Alustar</Name>          value
  <e d:type="sUInt64">5461248434221153178</e>          value without a name
  <InstanceGroup d:type="ref">0</InstanceGroup>        reference
  <Localisation-cUserLocalisedString> ... </...>       "::" is written as "-"
  <Other/>                                             element without children
</cRouteProperties>
```

(**Verified** for elements, values, ids and references.) How the game's own
tools write arrays, nil and blobs in XML is **assumed**; `opentsc` writes
`d:numElements`/`d:elementType`, `<d:nil/>` and `<d:blob d:size="..">hex</d:blob>`
and reads the same back. Hints are written as XML comments and are lost when
reading XML.

Because `-` also occurs inside ordinary names (GUIDs), XML names are not
converted back to `::`. Lookups treat the two spellings as equal instead.

## .ap archives

Plain ZIP archives; in the samples every entry is stored without
compression. Entry names use `/`. Lookups ignore letter case and slash
direction. (**Verified**)

## Blueprints.pak

A flat cache of every blueprint `.bin` of one product.

| Offset | Size | Content |
|-------:|-----:|---------|
| 0 | 4 | `2ULB` |
| 4 | 4 | version, 1 |
| 8 | 4 | entry count |
| 12 | 4 | total file size |
| 16 | 4 | size of the largest entry |
| 20 | 24 × count | entries |

Entry: name length u32, name offset u32, data size u32, data offset u32,
modification time u64 (Windows FILETIME). Names follow the table back to
back without terminators, using `\`; data follows the names. Every entry is a
SERZ file identical to the loose `.bin`. (**Verified** on two paks, 8 and 53
entries.)

## Route data

Route files are SERZ record sets: `cRecordSet/Record/<content>`.

### Coordinates

A route is cut into square tiles of 1024 m. Tile file names give the tile's
index as `±XXXXXX±ZZZZZZ.bin`. `x` grows to the east and `z` to the north:
Munich to Rosenheim runs towards +x and -z, and Rosenheim lies south-east of
Munich. (**Verified** on that route; the map of Sherman Hill also matches the
real line, with Cheyenne in the east.)

Positions are stored as a `cFarVector2` with, per axis, a whole tile count
(`RouteCoordinate`, sInt32) and metres inside the tile (`TileCoordinate`,
sFloat32). opentsc combines them into `f64` metres.

### Networks/Tracks.bin

`Network::cTrackNetwork` holds every *ribbon*: one stretch of track between
junctions or track ends. Per `Network::cTrackRibbon`:

- `_length`, in metres.
- `RibbonID`, a GUID.
- `Height`: points with `_position` (metres along the ribbon), `_height`
  (metres) and `_manual`.
- `Properties`: track rule, speed limits, signals, transfer points and more
  (not interpreted yet), each with `_start`/`_end` or `_offset` positions
  along the ribbon.

### Networks/Track Tiles

The centre-line geometry. Each tile lists, per ribbon, the curves that
**start** in that tile; a ribbon's curves are therefore spread over the
tiles it crosses and have to be put in order by matching each curve's end to
the next one's start. Curves stored under the all-zero GUID belong to no
ribbon and are unrelated to each other.

Every curve has `Length`, `StartPos` and `StartTangent` (a unit vector
`x z`). Three kinds exist in the samples:

**`cCurveStraight`**: nothing else.

**`cCurveArc`**: `Curvature` (1/m) and `CurveSign`. Sign +1
bends to the right, -1 to the left.

**`cCurveEasement`**: a transition spiral, with `Sharpness` (A), `Offset`
(d), `L0`, `CurveSign` and `TraversalSign` (T). It also stores `L0Offset`
and `L0Tangent`, the start point and direction in the spiral's own frame,
which is what gave the formula away.

The spiral is the clothoid's series cut off after the first terms. In its own
frame (`z` along the initial direction, `x` to the right), at parameter `p`:

```text
point      x = CurveSign · A p³ / 6          z = p − A² p⁵ / 40
direction  ( CurveSign · A p² / 2 ,  1 − A² p⁴ / 8 ), normalised
```

- The curve covers the parameters from `p(L0)` to `p(L0 + T · Length)`, so
  T = +1 runs into the bend and T = -1 runs out of it.
- With an offset, the point is moved sideways by `d · CurveSign · T` along
  the right-hand normal, and stored distances `u` relate to the parameter by
  `u = p − d · A · p² / 2`.
- For an offset easement with T = -1, `Length` is therefore not exactly the
  geometric length of the curve. Distances along ribbons still count in
  stored units.

(**Verified** by fit, see below. The offset rules are purely empirical: they
are the one combination of signs, out of 64 tried, that makes the curves
meet.)

### Terrain

`cHeightFieldTile` with `X`, `Y` (the tile index; `Y` is `z`) and one blob
of 65 536 bytes: 128 × 128 heights in metres as 32-bit floats. Samples are
8 m apart; rows run south to north and samples within a row west to east,
starting on the tile's south-west corner. A tile does not repeat its
neighbour's edge. (**Verified**: across about 950 pairs of neighbouring tiles,
the last two samples of a row or column extrapolate to the neighbour's first
with a median error of 1 to 3 cm, and no other pairing of edges comes close.)

### Not interpreted yet

- **Scenery tile**: `cDynamicEntity` records, each with a `cPosOri` (far
  matrix: tile coordinates plus offset), a `BlueprintID` triple and a GUID.
- **MixMap tile**: a 128 × 128 rectangle and one 98 304-byte blob, which fits
  6 bytes per sample.
- **GeoPcDx**: `cHcGeometry` with materials (shader name, textures by name),
  a scene graph with LOD selectors, primitive sets, and vertex and index
  buffers as blobs with a vertex declaration next to them.
- **TgPcDx**: `cHcTextureGroup/cHcTexture` with width, height and one
  `cHcImageDx` per mip level (format name, size, blob). A 64 × 64 mip of
  format `HC_IMAGE_FORMAT_COMPRESSED_INTERP_ALPHA` is 4 096 bytes, i.e. 16
  bytes per 4 × 4 block, which is the size of DXT3/DXT5 data.
- Junctions (`Network::cNetworkNode`), which connect the ribbons.

## How this was checked

Against a sample of about 1 GB taken from one installation: 7 `.ap` archives
(two packed routes, three train packs, two asset packs), 16 `Blueprints.pak`
files and 42 loose files.

- `opentsc scan` parses all 15 305 SERZ documents in the sample without
  error: 14 542 `.bin`, 542 `.TgPcDx`, 186 `.GeoPcDx`, 34 `.proxybin` and
  1 `.XSec`. Three of them close an element under the wrong name (see above).
- 30 binaries in the sample ship with their XML source. For 29 the decoded
  tree is identical to the source: same elements, types, ids and values. The
  30th (`DK_Skinner_Gammel_Ren`) is a different version from its source: the
  binary names a buffer blueprint and 14 rumble sounds where the source has
  empty fields and one.
- Six routes are listed with their names, two of them from `MainContent.ap`.

- Track geometry, on three complete routes (Munich to Rosenheim, Sherman
  Hill and a small unpacked route; 13 505 curves, 1 621 of them easements):
  - every curve is one of the three known kinds;
  - the curves of every ribbon assemble into one unbroken line;
  - at the 7 002 joints, the computed end of a curve and the stored start of
    the next are at most 11 mm apart (99% within 4 mm);
  - for all 6 466 ribbons that have curves, the curve lengths add up to the
    ribbon's `_length` within 5 cm.
  `opentsc map` prints these figures for any route.

Open questions from that check: some ribbons have no curves at all (20 on
Munich to Rosenheim, 2 on Sherman Hill; most have length 0, a few are 250 to
300 m long), and what the ribbon-less curves are for is not known.

This is a sample, not the whole installation. `opentsc scan <RailWorks folder>`
runs the file check over everything and lists each file that fails, and
`OPENTSC_RAILWORKS=... cargo test` runs the geometry check on every route.
