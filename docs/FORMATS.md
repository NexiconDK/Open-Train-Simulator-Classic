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
installed. A route's own files can also sit inside an `.ap` in its folder.
(**Assumed**: the sample installation's unpacked route was the only kind
inspected so far.)

Loose files take priority over files inside `.ap` archives. (**Assumed**:
this is the commonly described behaviour; not yet tested against the game.)

## SERZ

A compact binary encoding of an XML-like tree. Seen in `.bin`, `.proxybin`,
`.GeoPcDx` (models), `.TgPcDx` (textures) and all route and scenario `.bin`
files. Other files (`.ban` animations, `.out` compiled Lua, `.dav` audio) are
different formats.

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
  builds the tree from open/close pairs and checks that names match.
  (**Verified**)
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

## What the tree contains (first observations)

Not implemented yet; noted here so the next steps have a starting point.

- **Scenery tile**: `cRecordSet/Record/cDynamicEntity*`, each with a
  `cPosOri` (far matrix: tile coordinates plus offset), a `BlueprintID`
  triple and an entity GUID.
- **Tracks.bin**: `Network::cTrackNetwork` with `Network::cTrackRibbon`s
  (length, height points, properties such as speed limits, signals and
  transfer points).
- **Terrain tile**: tile X/Y and one 65 536-byte blob, which fits 128 × 128
  samples of 4 bytes.
- **MixMap tile**: a 128 × 128 rectangle and one 98 304-byte blob, which fits
  6 bytes per sample.
- **GeoPcDx**: `cHcGeometry` with materials (shader name, textures by name),
  a scene graph with LOD selectors, primitive sets, and vertex and index
  buffers as blobs with a vertex declaration next to them.
- **TgPcDx**: `cHcTextureGroup/cHcTexture` with width, height and one
  `cHcImageDx` per mip level (format name, size, blob). A 64 × 64 mip of
  format `HC_IMAGE_FORMAT_COMPRESSED_INTERP_ALPHA` is 4 096 bytes, i.e. 16
  bytes per 4 × 4 block, which is the size of DXT3/DXT5 data.

## How this was checked

- 834 SERZ files from one installation parse without error: 21 loose files,
  the 61 entries of two `Blueprints.pak` files, and all 752 SERZ entries of
  one `.ap` archive (458 blueprints, 118 models, 176 textures).
- For three files that ship with their XML source (two scenery blueprints and
  a 454-element sound proxy) the decoded tree is identical to the source:
  same elements, types, ids and values.
- `opentsc scan <RailWorks folder>` runs the same check over a whole
  installation and lists every file that fails.
