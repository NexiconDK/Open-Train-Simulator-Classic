//! Reader for the `SERZ` binary container.
//!
//! `SERZ` is a compact binary encoding of an XML-like tree. The game uses it
//! for blueprints (`.bin`), sound definitions (`.proxybin`), models
//! (`.GeoPcDx`), textures (`.TgPcDx`) and route, scenery and scenario data.
//!
//! The byte-level layout is described in `docs/FORMATS.md`. In short: after an
//! 8-byte header the file is a stream of *lines*. A line is introduced either
//! by `0xFF <command>` followed by its definition, or by a single byte below
//! `0xFF` that repeats one of the last 255 defined lines with new data.
//! Strings are written once and referred to by a 16-bit index afterwards.

use crate::error::{Error, Result};
use crate::node::{Document, Element, Node, ScalarType, Value};

/// First four bytes of every `SERZ` file.
pub const MAGIC: &[u8; 4] = b"SERZ";

/// Number of line definitions the format keeps for back-references.
const LINE_CACHE: usize = 255;

/// Nesting deeper than this is treated as a corrupt file. Real files stay
/// below 40 levels.
const MAX_DEPTH: usize = 512;

const CMD_OPEN: u8 = 0x50; // 'P'
const CMD_VALUE: u8 = 0x56; // 'V'
const CMD_CLOSE: u8 = 0x70; // 'p'
const CMD_ARRAY: u8 = 0x41; // 'A'
const CMD_NIL: u8 = 0x4E; // 'N'
const CMD_REFERENCE: u8 = 0x52; // 'R'
const CMD_BLOB: u8 = 0x42; // 'B'
const CMD_HINT: u8 = 0x43; // 'C'

/// True if `data` starts with the `SERZ` magic.
pub fn is_serz(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == MAGIC
}

/// Parse a complete `SERZ` file.
pub fn parse(data: &[u8]) -> Result<Document> {
    Parser::new(data)?.run()
}

/// One remembered line definition: everything about a line except its data.
#[derive(Clone)]
enum Line {
    Open { name: u32 },
    Value { name: u32, ty: ScalarType },
    Close { name: u32 },
    Array { name: u32, ty: ScalarType },
    Reference { name: u32 },
    Nil,
    Blob,
    Hint,
}

struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
    /// Every string defined so far; later uses refer to it by index.
    strings: Vec<String>,
    /// Ring of the last [`LINE_CACHE`] line definitions.
    lines: Vec<Line>,
    lines_defined: usize,
}

impl<'a> Parser<'a> {
    fn new(data: &'a [u8]) -> Result<Self> {
        if !is_serz(data) {
            return Err(Error::malformed(0, "missing SERZ magic"));
        }
        if data.len() < 8 {
            return Err(Error::malformed(4, "truncated header"));
        }
        // Bytes 4..8 are 00 00 01 00 in every file seen; treated as a version.
        Ok(Parser {
            data,
            pos: 8,
            strings: Vec::new(),
            lines: Vec::new(),
            lines_defined: 0,
        })
    }

    fn run(mut self) -> Result<Document> {
        let mut top: Vec<Node> = Vec::new();
        let mut stack: Vec<Element> = Vec::new();

        while self.pos < self.data.len() {
            let line_start = self.pos;
            let line = self.next_line()?;
            let node = match line {
                Line::Open { name } => {
                    let id = self.u32()?;
                    let _declared_children = self.u32()?; // not reliable, see FORMATS.md
                    if stack.len() >= MAX_DEPTH {
                        return Err(Error::malformed(line_start, "nesting too deep"));
                    }
                    stack.push(Element {
                        name: self.name(name),
                        id,
                        children: Vec::new(),
                    });
                    continue;
                }
                Line::Close { name } => {
                    let element = stack
                        .pop()
                        .ok_or_else(|| Error::malformed(line_start, "close without open"))?;
                    if element.name != self.strings[name as usize] {
                        return Err(Error::malformed(
                            line_start,
                            format!(
                                "close of '{}' while '{}' is open",
                                self.strings[name as usize], element.name
                            ),
                        ));
                    }
                    Node::Element(element)
                }
                Line::Value { name, ty } => {
                    let value = self.value(ty)?;
                    Node::Value {
                        name: self.name(name),
                        value,
                    }
                }
                Line::Array { name, ty } => {
                    let count = self.u8()? as usize;
                    let mut values = Vec::with_capacity(count);
                    for _ in 0..count {
                        values.push(self.value(ty)?);
                    }
                    Node::Array {
                        name: self.name(name),
                        elem_type: ty,
                        values,
                    }
                }
                Line::Reference { name } => {
                    let target = self.u32()?;
                    Node::Reference {
                        name: self.name(name),
                        target,
                    }
                }
                Line::Nil => Node::Nil,
                Line::Blob => {
                    let size = self.u32()? as usize;
                    Node::Blob(self.take(size)?.to_vec())
                }
                Line::Hint => {
                    let flag = self.u8()?;
                    let value = self.u32()?;
                    Node::Hint { flag, value }
                }
            };
            match stack.last_mut() {
                Some(parent) => parent.children.push(node),
                None => top.push(node),
            }
        }

        if let Some(open) = stack.last() {
            return Err(Error::malformed(
                self.pos,
                format!("file ends while '{}' is still open", open.name),
            ));
        }
        Ok(Document { nodes: top })
    }

    /// Read the introduction of the next line: a fresh definition or a
    /// back-reference to a cached one.
    fn next_line(&mut self) -> Result<Line> {
        let at = self.pos;
        let first = self.u8()?;
        if first != 0xFF {
            return self.lines.get(first as usize).cloned().ok_or_else(|| {
                Error::malformed(at, format!("back-reference to undefined line {first}"))
            });
        }
        let command = self.u8()?;
        let line = match command {
            CMD_OPEN => Line::Open {
                name: self.string_index()?,
            },
            CMD_VALUE => {
                let name = self.string_index()?;
                Line::Value {
                    name,
                    ty: self.scalar_type()?,
                }
            }
            CMD_CLOSE => Line::Close {
                name: self.string_index()?,
            },
            CMD_ARRAY => {
                let name = self.string_index()?;
                Line::Array {
                    name,
                    ty: self.scalar_type()?,
                }
            }
            CMD_REFERENCE => Line::Reference {
                name: self.string_index()?,
            },
            CMD_NIL => Line::Nil,
            CMD_BLOB => Line::Blob,
            CMD_HINT => Line::Hint,
            other => {
                return Err(Error::malformed(
                    at,
                    format!("unknown command {other:#04x}"),
                ))
            }
        };
        // Every definition takes the next slot of the ring, whatever its kind.
        let slot = self.lines_defined % LINE_CACHE;
        if slot < self.lines.len() {
            self.lines[slot] = line.clone();
        } else {
            self.lines.push(line.clone());
        }
        self.lines_defined += 1;
        Ok(line)
    }

    fn name(&self, index: u32) -> String {
        self.strings[index as usize].clone()
    }

    fn scalar_type(&mut self) -> Result<ScalarType> {
        let at = self.pos;
        let index = self.string_index()?;
        let name = &self.strings[index as usize];
        ScalarType::from_name(name)
            .ok_or_else(|| Error::malformed(at, format!("unknown value type '{name}'")))
    }

    fn value(&mut self, ty: ScalarType) -> Result<Value> {
        Ok(match ty {
            ScalarType::Bool => Value::Bool(self.u8()? != 0),
            ScalarType::I8 => Value::I8(self.u8()? as i8),
            ScalarType::U8 => Value::U8(self.u8()?),
            ScalarType::I16 => Value::I16(i16::from_le_bytes(self.array()?)),
            ScalarType::U16 => Value::U16(u16::from_le_bytes(self.array()?)),
            ScalarType::I32 => Value::I32(i32::from_le_bytes(self.array()?)),
            ScalarType::U32 => Value::U32(u32::from_le_bytes(self.array()?)),
            ScalarType::I64 => Value::I64(i64::from_le_bytes(self.array()?)),
            ScalarType::U64 => Value::U64(u64::from_le_bytes(self.array()?)),
            ScalarType::F32 => Value::F32(f32::from_le_bytes(self.array()?)),
            ScalarType::F64 => Value::F64(f64::from_le_bytes(self.array()?)),
            ScalarType::String => {
                let index = self.string_index()?;
                Value::String(self.name(index))
            }
        })
    }

    /// Read a string and return its index in the string table.
    ///
    /// `FF FF` introduces a new string (which is appended to the table);
    /// anything else is the 16-bit index of an earlier one.
    fn string_index(&mut self) -> Result<u32> {
        let at = self.pos;
        let index = u16::from_le_bytes(self.array()?);
        if index != 0xFFFF {
            return if (index as usize) < self.strings.len() {
                Ok(index as u32)
            } else {
                Err(Error::malformed(
                    at,
                    format!("reference to undefined string {index}"),
                ))
            };
        }
        let chars = self.u32()? as usize;
        let text = self.text(chars)?;
        self.strings.push(text);
        Ok((self.strings.len() - 1) as u32)
    }

    /// Read `chars` characters of UTF-8 text.
    ///
    /// The length prefix counts characters, not bytes: "Übergabe1" is stored
    /// with length 9 followed by 10 bytes. Bytes that are not valid UTF-8 are
    /// taken as single Latin-1 characters so that old content still loads.
    fn text(&mut self, chars: usize) -> Result<String> {
        if chars > self.data.len() - self.pos {
            return Err(Error::malformed(self.pos, "string runs past end of file"));
        }
        let mut out = String::with_capacity(chars);
        for _ in 0..chars {
            let lead = self.u8()?;
            let extra = match lead {
                0x00..=0x7F => 0,
                0xC0..=0xDF => 1,
                0xE0..=0xEF => 2,
                0xF0..=0xF7 => 3,
                _ => 0,
            };
            let end = self.pos + extra;
            let decoded = (extra > 0 && end <= self.data.len())
                .then(|| std::str::from_utf8(&self.data[self.pos - 1..end]).ok())
                .flatten();
            match decoded {
                Some(s) => {
                    out.push_str(s);
                    self.pos = end;
                }
                None => out.push(lead as char),
            }
        }
        Ok(out)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        if len > self.data.len() - self.pos {
            return Err(Error::malformed(self.pos, "unexpected end of file"));
        }
        let slice = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tiny builder for hand-made test files, so the tests need no game data.
    #[derive(Default)]
    struct Bytes(Vec<u8>);

    impl Bytes {
        fn header() -> Self {
            Bytes(b"SERZ\x00\x00\x01\x00".to_vec())
        }
        fn raw(mut self, bytes: &[u8]) -> Self {
            self.0.extend_from_slice(bytes);
            self
        }
        fn new_str(mut self, s: &str) -> Self {
            self.0.extend_from_slice(&[0xFF, 0xFF]);
            self.0
                .extend_from_slice(&(s.chars().count() as u32).to_le_bytes());
            self.0.extend_from_slice(s.as_bytes());
            self
        }
        fn str_ref(self, index: u16) -> Self {
            self.raw(&index.to_le_bytes())
        }
        fn u32(self, v: u32) -> Self {
            self.raw(&v.to_le_bytes())
        }
    }

    #[test]
    fn rejects_non_serz() {
        assert!(parse(b"nope").is_err());
        assert!(parse(b"SERZ").is_err());
    }

    #[test]
    fn element_with_values_and_backrefs() {
        // <Root id=7>
        //   <Name type=cDeltaString>Übergabe1</Name>   (new definition)
        //   <Name type=cDeltaString>Übergabe1</Name>   (back-reference to line 1)
        //   <Count type=sInt32>-2</Count>
        // </Root>
        let data = Bytes::header()
            .raw(&[0xFF, CMD_OPEN])
            .new_str("Root")
            .u32(7)
            .u32(3) // line 0
            .raw(&[0xFF, CMD_VALUE])
            .new_str("Name")
            .new_str("cDeltaString") // line 1
            .new_str("Übergabe1") // string 3
            .raw(&[0x01])
            .str_ref(3) // repeat line 1
            .raw(&[0xFF, CMD_VALUE])
            .new_str("Count")
            .new_str("sInt32") // line 2
            .raw(&(-2i32).to_le_bytes())
            .raw(&[0xFF, CMD_CLOSE])
            .str_ref(0) // line 3
            .0;
        let doc = parse(&data).unwrap();
        let root = doc.root().unwrap();
        assert_eq!(root.name, "Root");
        assert_eq!(root.id, 7);
        assert_eq!(root.children.len(), 3);
        assert_eq!(root.string("Name"), Some("Übergabe1"));
        assert_eq!(root.children[0], root.children[1]);
        assert_eq!(root.value("Count"), Some(&Value::I32(-2)));
    }

    #[test]
    fn array_reference_nil_blob_hint() {
        let data = Bytes::header()
            .raw(&[0xFF, CMD_HINT, 0x00])
            .u32(12) // line 0
            .raw(&[0xFF, CMD_OPEN])
            .new_str("R")
            .u32(0)
            .u32(4) // line 1
            .raw(&[0xFF, CMD_ARRAY])
            .new_str("Pos")
            .new_str("sFloat32") // line 2
            .raw(&[2])
            .raw(&1.5f32.to_le_bytes())
            .raw(&(-4.0f32).to_le_bytes())
            .raw(&[0xFF, CMD_REFERENCE])
            .new_str("Target")
            .u32(99) // line 3
            .raw(&[0xFF, CMD_NIL]) // line 4
            .raw(&[0xFF, CMD_BLOB])
            .u32(3)
            .raw(&[1, 2, 3]) // line 5
            .raw(&[0x05])
            .u32(2)
            .raw(&[9, 8]) // repeat blob line
            .raw(&[0xFF, CMD_CLOSE])
            .str_ref(0) // line 6
            .0;
        let doc = parse(&data).unwrap();
        assert_eq!(doc.nodes[0], Node::Hint { flag: 0, value: 12 });
        let root = doc.root().unwrap();
        assert_eq!(
            root.array("Pos"),
            Some(&[Value::F32(1.5), Value::F32(-4.0)][..])
        );
        assert_eq!(
            root.children[1],
            Node::Reference {
                name: "Target".into(),
                target: 99
            }
        );
        assert_eq!(root.children[2], Node::Nil);
        assert_eq!(root.children[3], Node::Blob(vec![1, 2, 3]));
        assert_eq!(root.children[4], Node::Blob(vec![9, 8]));
    }

    #[test]
    fn line_cache_wraps_after_255_definitions() {
        // Define 256 value lines; the 256th overwrites slot 0, so a
        // back-reference to slot 0 must yield the last definition.
        let mut b = Bytes::header()
            .raw(&[0xFF, CMD_OPEN])
            .new_str("R")
            .u32(0)
            .u32(0); // slot 0
        b = b
            .raw(&[0xFF, CMD_VALUE])
            .new_str("v1")
            .new_str("sUInt8")
            .raw(&[1]); // slot 1
        for i in 2..255u32 {
            b = b
                .raw(&[0xFF, CMD_VALUE])
                .new_str(&format!("v{i}"))
                .str_ref(2)
                .raw(&[i as u8]);
        }
        b = b
            .raw(&[0xFF, CMD_VALUE])
            .new_str("last")
            .str_ref(2)
            .raw(&[77]); // wraps into slot 0
        b = b.raw(&[0x00, 78]); // repeat slot 0 -> "last"
        b = b.raw(&[0xFF, CMD_CLOSE]).str_ref(0);
        let doc = parse(&b.0).unwrap();
        let root = doc.root().unwrap();
        let n = root.children.len();
        assert_eq!(
            root.children[n - 1],
            Node::Value {
                name: "last".into(),
                value: Value::U8(78)
            }
        );
    }

    #[test]
    fn mismatched_close_is_an_error() {
        let data = Bytes::header()
            .raw(&[0xFF, CMD_OPEN])
            .new_str("A")
            .u32(0)
            .u32(0)
            .raw(&[0xFF, CMD_OPEN])
            .new_str("B")
            .u32(0)
            .u32(0)
            .raw(&[0xFF, CMD_CLOSE])
            .str_ref(0)
            .0;
        assert!(parse(&data).is_err());
    }

    #[test]
    fn truncated_input_is_an_error_not_a_panic() {
        let data = Bytes::header()
            .raw(&[0xFF, CMD_OPEN])
            .new_str("Root")
            .u32(7)
            .u32(3)
            .raw(&[0xFF, CMD_VALUE])
            .new_str("Name")
            .new_str("cDeltaString")
            .new_str("abc")
            .raw(&[0xFF, CMD_CLOSE])
            .str_ref(0)
            .0;
        // A bare 8-byte header is a valid, empty document, so start after it.
        assert!(parse(&data[..8]).unwrap().nodes.is_empty());
        for cut in (0..data.len()).filter(|&c| c != 8) {
            assert!(parse(&data[..cut]).is_err(), "prefix of {cut} bytes parsed");
        }
        assert!(parse(&data).is_ok());
    }
}
