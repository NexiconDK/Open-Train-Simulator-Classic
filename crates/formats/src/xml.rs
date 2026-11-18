//! The XML encoding of a [`Document`].
//!
//! Some game files are stored as XML instead of `SERZ` (`RouteProperties.xml`,
//! `ScenarioProperties.xml`, blueprint sources). Both encodings describe the
//! same tree, so both are read into the same [`Document`] type.
//!
//! The writer follows the conventions seen in the game's own XML files:
//!
//! ```xml
//! <cBlueprintLoader xmlns:d="http://www.kuju.com/TnT/2003/Delta" d:version="1.0">
//!   <Name d:type="cDeltaString">Alustar</Name>
//!   <kLoud-cSingleSampleSound d:id="126582">...</kLoud-cSingleSampleSound>
//!   <InstanceGroup d:type="ref">0</InstanceGroup>
//! </cBlueprintLoader>
//! ```

use std::fmt::Write as _;

use crate::error::{Error, Result};
use crate::node::{Document, Element, Node, ScalarType, Value};

/// Namespace of the `d:` attributes.
pub const DELTA_NS: &str = "http://www.kuju.com/TnT/2003/Delta";

/// Tag used for values and arrays that have no name of their own.
const ANONYMOUS: &str = "e";

/// Choices for [`to_string_with`].
#[derive(Debug, Clone, Copy, Default)]
pub struct WriteOptions {
    /// Replace the hex dump of each blob with a comment giving its size.
    /// Handy for reading model and texture files; the result no longer
    /// carries the data.
    pub elide_blobs: bool,
}

/// Render a document as XML text.
pub fn to_string(doc: &Document) -> String {
    to_string_with(doc, WriteOptions::default())
}

/// Render a document as XML text with the given options.
pub fn to_string_with(doc: &Document, options: WriteOptions) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    let mut first_element = true;
    for node in &doc.nodes {
        let is_root = first_element && matches!(node, Node::Element(_));
        write_node(&mut out, node, 0, is_root, options);
        first_element &= !is_root;
    }
    out
}

fn write_node(out: &mut String, node: &Node, depth: usize, is_root: bool, options: WriteOptions) {
    indent(out, depth);
    match node {
        Node::Element(e) => write_element(out, e, depth, is_root, options),
        Node::Value { name, value } => {
            let tag = tag_name(name);
            let _ = write!(out, "<{tag} d:type=\"{}\">", value.scalar_type().name());
            escape_into(out, &value.to_string(), false);
            let _ = writeln!(out, "</{tag}>");
        }
        Node::Array {
            name,
            elem_type,
            values,
        } => {
            let tag = tag_name(name);
            let _ = write!(
                out,
                "<{tag} d:numElements=\"{}\" d:elementType=\"{}\">",
                values.len(),
                elem_type.name()
            );
            if *elem_type == ScalarType::String {
                // Strings may contain spaces, so they get one child each.
                out.push('\n');
                for v in values {
                    indent(out, depth + 1);
                    out.push_str("<e>");
                    escape_into(out, &v.to_string(), false);
                    out.push_str("</e>\n");
                }
                indent(out, depth);
            } else {
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    let _ = write!(out, "{v}");
                }
            }
            let _ = writeln!(out, "</{tag}>");
        }
        Node::Reference { name, target } => {
            let tag = tag_name(name);
            let _ = writeln!(out, "<{tag} d:type=\"ref\">{target}</{tag}>");
        }
        Node::Nil => out.push_str("<d:nil/>\n"),
        Node::Blob(bytes) if options.elide_blobs => {
            let _ = writeln!(out, "<!-- blob of {} bytes -->", bytes.len());
        }
        Node::Blob(bytes) => {
            let _ = write!(out, "<d:blob d:size=\"{}\">", bytes.len());
            for (i, b) in bytes.iter().enumerate() {
                if i % 32 == 0 {
                    out.push('\n');
                    indent(out, depth + 1);
                }
                let _ = write!(out, "{b:02x}");
            }
            out.push('\n');
            indent(out, depth);
            out.push_str("</d:blob>\n");
        }
        Node::Hint { flag, value } => {
            let _ = writeln!(out, "<!-- opentsc:hint flag={flag} value={value} -->");
        }
    }
}

fn write_element(
    out: &mut String,
    e: &Element,
    depth: usize,
    is_root: bool,
    options: WriteOptions,
) {
    let tag = tag_name(&e.name);
    let _ = write!(out, "<{tag}");
    if is_root {
        let _ = write!(out, " xmlns:d=\"{DELTA_NS}\" d:version=\"1.0\"");
    }
    if e.id != 0 {
        let _ = write!(out, " d:id=\"{}\"", e.id);
    }
    if e.children.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push_str(">\n");
    for child in &e.children {
        write_node(out, child, depth + 1, false, options);
    }
    indent(out, depth);
    let _ = writeln!(out, "</{tag}>");
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push('\t');
    }
}

/// Binary names use `::` for C++ scopes, which is not legal in an XML tag.
fn tag_name(name: &str) -> String {
    if name.is_empty() {
        ANONYMOUS.to_string()
    } else {
        name.replace("::", "-")
    }
}

fn escape_into(out: &mut String, text: &str, attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            // Control characters other than tab/newline are not allowed in XML 1.0.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {
                let _ = write!(out, "&#x{:X};", c as u32);
            }
            c => out.push(c),
        }
    }
}

/// Parse XML bytes (UTF-8, with or without a byte-order mark).
pub fn parse_bytes(data: &[u8]) -> Result<Document> {
    let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
    let text = std::str::from_utf8(data).map_err(|e| Error::Xml(format!("not UTF-8: {e}")))?;
    parse_str(text)
}

/// Parse XML text into a [`Document`].
///
/// Tag names are kept as written (`Localisation-cUserLocalisedString`); the
/// lookup helpers on [`Element`] treat `-` and `::` alike.
pub fn parse_str(text: &str) -> Result<Document> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let xml = roxmltree::Document::parse_with_options(text, options)
        .map_err(|e| Error::Xml(e.to_string()))?;
    Ok(Document {
        nodes: vec![convert(xml.root_element())?],
    })
}

fn convert(node: roxmltree::Node<'_, '_>) -> Result<Node> {
    let tag = node.tag_name();
    let in_delta = tag.namespace() == Some(DELTA_NS);
    if in_delta && tag.name() == "nil" {
        return Ok(Node::Nil);
    }
    if in_delta && tag.name() == "blob" {
        return Ok(Node::Blob(parse_hex(node.text().unwrap_or(""))?));
    }

    let name = if tag.name() == ANONYMOUS && has_delta_type(node) {
        String::new()
    } else {
        tag.name().to_string()
    };
    let text = node.text().unwrap_or("");

    if let Some(ty) = node.attribute((DELTA_NS, "type")) {
        if ty == "ref" {
            let target = text
                .trim()
                .parse()
                .map_err(|_| bad_value(&name, ty, text))?;
            return Ok(Node::Reference { name, target });
        }
        let scalar = ScalarType::from_name(ty)
            .ok_or_else(|| Error::Xml(format!("<{}>: unknown d:type '{ty}'", tag.name())))?;
        let value = parse_value(scalar, text).ok_or_else(|| bad_value(&name, ty, text))?;
        return Ok(Node::Value { name, value });
    }

    if let Some(ty) = node.attribute((DELTA_NS, "elementType")) {
        let scalar = ScalarType::from_name(ty)
            .ok_or_else(|| Error::Xml(format!("<{}>: unknown d:elementType '{ty}'", tag.name())))?;
        let values = if scalar == ScalarType::String {
            node.children()
                .filter(|c| c.is_element())
                .map(|c| Value::String(c.text().unwrap_or("").to_string()))
                .collect()
        } else {
            text.split_whitespace()
                .map(|t| parse_value(scalar, t).ok_or_else(|| bad_value(&name, ty, t)))
                .collect::<Result<Vec<_>>>()?
        };
        return Ok(Node::Array {
            name,
            elem_type: scalar,
            values,
        });
    }

    let id = match node.attribute((DELTA_NS, "id")) {
        Some(id) => id.trim().parse().map_err(|_| bad_value(&name, "id", id))?,
        None => 0,
    };
    let children = node
        .children()
        .filter(|c| c.is_element())
        .map(convert)
        .collect::<Result<Vec<_>>>()?;
    Ok(Node::Element(Element { name, id, children }))
}

fn has_delta_type(node: roxmltree::Node<'_, '_>) -> bool {
    node.attribute((DELTA_NS, "type")).is_some()
        || node.attribute((DELTA_NS, "elementType")).is_some()
}

fn bad_value(name: &str, ty: &str, text: &str) -> Error {
    Error::Xml(format!("<{name}>: '{text}' is not a valid {ty}"))
}

fn parse_value(ty: ScalarType, text: &str) -> Option<Value> {
    let t = text.trim();
    Some(match ty {
        ScalarType::Bool => Value::Bool(match t {
            "1" | "true" | "True" => true,
            "0" | "false" | "False" | "" => false,
            _ => return None,
        }),
        ScalarType::I8 => Value::I8(t.parse().ok()?),
        ScalarType::U8 => Value::U8(t.parse().ok()?),
        ScalarType::I16 => Value::I16(t.parse().ok()?),
        ScalarType::U16 => Value::U16(t.parse().ok()?),
        ScalarType::I32 => Value::I32(t.parse().ok()?),
        ScalarType::U32 => Value::U32(t.parse().ok()?),
        ScalarType::I64 => Value::I64(t.parse().ok()?),
        ScalarType::U64 => Value::U64(t.parse().ok()?),
        ScalarType::F32 => Value::F32(t.parse().ok()?),
        ScalarType::F64 => Value::F64(t.parse().ok()?),
        // Strings keep their whitespace exactly.
        ScalarType::String => Value::String(text.to_string()),
    })
}

fn parse_hex(text: &str) -> Result<Vec<u8>> {
    let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if digits.len() % 2 != 0 {
        return Err(Error::Xml("blob has an odd number of hex digits".into()));
    }
    digits
        .chunks(2)
        .map(|pair| {
            let hi = (pair[0] as char).to_digit(16);
            let lo = (pair[1] as char).to_digit(16);
            match (hi, lo) {
                (Some(h), Some(l)) => Ok((h * 16 + l) as u8),
                _ => Err(Error::Xml("blob contains a non-hex character".into())),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Document {
        Document {
            nodes: vec![Node::Element(Element {
                name: "cRecordSet".into(),
                id: 42,
                children: vec![
                    Node::Value {
                        name: "Name".into(),
                        value: Value::String("a < b & c".into()),
                    },
                    Node::Value {
                        name: String::new(),
                        value: Value::U64(18446744073709551615),
                    },
                    Node::Value {
                        name: "Flag".into(),
                        value: Value::Bool(true),
                    },
                    Node::Array {
                        name: "Pos".into(),
                        elem_type: ScalarType::F32,
                        values: vec![Value::F32(1.5), Value::F32(-0.25), Value::F32(1e9)],
                    },
                    Node::Reference {
                        name: "Target".into(),
                        target: 7,
                    },
                    Node::Nil,
                    Node::Blob(vec![0, 1, 254, 255]),
                    Node::Element(Element {
                        name: "Network::cTrackRibbon".into(),
                        id: 0,
                        children: vec![],
                    }),
                ],
            })],
        }
    }

    #[test]
    fn round_trips_through_xml() {
        let doc = sample();
        let text = to_string(&doc);
        let back = parse_str(&text).unwrap();
        // Scope separators come back in their XML spelling.
        let root = back.root().unwrap();
        assert!(root.child("Network::cTrackRibbon").is_some());
        assert_eq!(root.children[..7], doc.root().unwrap().children[..7]);
        assert_eq!(root.id, 42);
    }

    #[test]
    fn reads_game_style_xml() {
        let text = r#"<?xml version="1.0" encoding="utf-8"?>
<cRouteProperties xmlns:d="http://www.kuju.com/TnT/2003/Delta" d:version="1.0" d:id="134209024">
	<ID><cGUID><UUID>
		<e d:type="sUInt64">5461248434221153178</e>
		<e d:type="sUInt64">15984781352438301312</e>
	</UUID></cGUID></ID>
	<DisplayName>
		<Localisation-cUserLocalisedString>
			<English d:type="cDeltaString"></English>
			<German d:type="cDeltaString">Demo</German>
			<Other/>
		</Localisation-cUserLocalisedString>
	</DisplayName>
	<InfrastructureModified d:type="bool">1</InfrastructureModified>
</cRouteProperties>"#;
        let doc = parse_str(text).unwrap();
        let root = doc.root().unwrap();
        assert_eq!(root.id, 134209024);
        let names = root
            .path(&["DisplayName", "Localisation::cUserLocalisedString"])
            .unwrap();
        assert_eq!(names.string("German"), Some("Demo"));
        assert_eq!(names.string("English"), Some(""));
        assert_eq!(
            root.value("InfrastructureModified"),
            Some(&Value::Bool(true))
        );
        let uuid = root.find("UUID").unwrap();
        assert_eq!(
            uuid.children[1],
            Node::Value {
                name: String::new(),
                value: Value::U64(15984781352438301312),
            }
        );
    }
}
