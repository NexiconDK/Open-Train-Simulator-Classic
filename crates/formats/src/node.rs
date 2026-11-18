//! The document tree shared by the binary (`SERZ`) and XML encodings.

use std::fmt;

/// A parsed file: the nodes found at the top level, in file order.
///
/// Almost every file has exactly one top-level [`Element`], sometimes preceded
/// by a [`Node::Hint`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    pub nodes: Vec<Node>,
}

impl Document {
    /// The first top-level element, which is the root in every file seen so far.
    pub fn root(&self) -> Option<&Element> {
        self.nodes.iter().find_map(Node::as_element)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A named container of further nodes.
    Element(Element),
    /// A named scalar, e.g. `<Name d:type="cDeltaString">Alustar</Name>`.
    Value { name: String, value: Value },
    /// A named fixed-type list of scalars, e.g. a position or a matrix row.
    Array {
        name: String,
        elem_type: ScalarType,
        values: Vec<Value>,
    },
    /// A pointer to the element whose `id` equals `target` (0 = null).
    Reference { name: String, target: u32 },
    /// An explicit "nothing here" placeholder inside a list.
    Nil,
    /// Raw bytes: height maps, vertex buffers, texture mip levels.
    Blob(Vec<u8>),
    /// Binary command `0x43`. Its meaning is not known yet; it looks like a
    /// size hint for the loader. Kept so nothing is lost.
    Hint { flag: u8, value: u32 },
}

impl Node {
    pub fn as_element(&self) -> Option<&Element> {
        match self {
            Node::Element(e) => Some(e),
            _ => None,
        }
    }

    /// The node's name, for the kinds that have one.
    pub fn name(&self) -> Option<&str> {
        match self {
            Node::Element(e) => Some(&e.name),
            Node::Value { name, .. } | Node::Array { name, .. } | Node::Reference { name, .. } => {
                Some(name)
            }
            Node::Nil | Node::Blob(_) | Node::Hint { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
    /// Class or field name. The binary encoding writes C++ scopes as `::`,
    /// the XML encoding writes them as `-`; lookups accept either.
    pub name: String,
    /// Identity used by [`Node::Reference`]. 0 means "not referenced".
    pub id: u32,
    pub children: Vec<Node>,
}

impl Element {
    /// First child element called `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children
            .iter()
            .filter_map(Node::as_element)
            .find(|e| name_eq(&e.name, name))
    }

    /// All child elements, whatever their names.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(Node::as_element)
    }

    /// Follow a chain of child element names.
    pub fn path(&self, names: &[&str]) -> Option<&Element> {
        names.iter().try_fold(self, |e, n| e.child(n))
    }

    /// The scalar child called `name`.
    pub fn value(&self, name: &str) -> Option<&Value> {
        self.children.iter().find_map(|n| match n {
            Node::Value { name: n, value } if name_eq(n, name) => Some(value),
            _ => None,
        })
    }

    /// The string child called `name`.
    pub fn string(&self, name: &str) -> Option<&str> {
        match self.value(name)? {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The array child called `name`.
    pub fn array(&self, name: &str) -> Option<&[Value]> {
        self.children.iter().find_map(|n| match n {
            Node::Array {
                name: n, values, ..
            } if name_eq(n, name) => Some(values.as_slice()),
            _ => None,
        })
    }

    /// Depth-first search for the first descendant element called `name`.
    pub fn find(&self, name: &str) -> Option<&Element> {
        let mut stack: Vec<&Element> = vec![self];
        while let Some(e) = stack.pop() {
            if name_eq(&e.name, name) {
                return Some(e);
            }
            // Reverse so the leftmost child is visited first.
            stack.extend(e.children.iter().rev().filter_map(Node::as_element));
        }
        None
    }
}

/// Compare two node names, treating the binary `::` and the XML `-` scope
/// separators as the same thing.
pub fn name_eq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if !(a.contains("::") || b.contains("::")) {
        return false;
    }
    a.replace("::", "-") == b.replace("::", "-")
}

/// The scalar types a [`Node::Value`] or [`Node::Array`] can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScalarType {
    Bool,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    String,
}

impl ScalarType {
    /// Parse the type name the game uses (`sInt32`, `cDeltaString`, ...).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "bool" => ScalarType::Bool,
            "sInt8" => ScalarType::I8,
            "sUInt8" => ScalarType::U8,
            "sInt16" => ScalarType::I16,
            "sUInt16" => ScalarType::U16,
            "sInt32" => ScalarType::I32,
            "sUInt32" => ScalarType::U32,
            "sInt64" => ScalarType::I64,
            "sUInt64" => ScalarType::U64,
            "sFloat32" => ScalarType::F32,
            "sFloat64" => ScalarType::F64,
            "cDeltaString" => ScalarType::String,
            _ => return None,
        })
    }

    /// The type name the game uses.
    pub fn name(self) -> &'static str {
        match self {
            ScalarType::Bool => "bool",
            ScalarType::I8 => "sInt8",
            ScalarType::U8 => "sUInt8",
            ScalarType::I16 => "sInt16",
            ScalarType::U16 => "sUInt16",
            ScalarType::I32 => "sInt32",
            ScalarType::U32 => "sUInt32",
            ScalarType::I64 => "sInt64",
            ScalarType::U64 => "sUInt64",
            ScalarType::F32 => "sFloat32",
            ScalarType::F64 => "sFloat64",
            ScalarType::String => "cDeltaString",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    String(String),
}

impl Value {
    pub fn scalar_type(&self) -> ScalarType {
        match self {
            Value::Bool(_) => ScalarType::Bool,
            Value::I8(_) => ScalarType::I8,
            Value::U8(_) => ScalarType::U8,
            Value::I16(_) => ScalarType::I16,
            Value::U16(_) => ScalarType::U16,
            Value::I32(_) => ScalarType::I32,
            Value::U32(_) => ScalarType::U32,
            Value::I64(_) => ScalarType::I64,
            Value::U64(_) => ScalarType::U64,
            Value::F32(_) => ScalarType::F32,
            Value::F64(_) => ScalarType::F64,
            Value::String(_) => ScalarType::String,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Any numeric value as `f64`. `u64`/`i64` beyond 2^53 lose precision.
    pub fn as_f64(&self) -> Option<f64> {
        Some(match *self {
            Value::I8(v) => v as f64,
            Value::U8(v) => v as f64,
            Value::I16(v) => v as f64,
            Value::U16(v) => v as f64,
            Value::I32(v) => v as f64,
            Value::U32(v) => v as f64,
            Value::I64(v) => v as f64,
            Value::U64(v) => v as f64,
            Value::F32(v) => v as f64,
            Value::F64(v) => v,
            Value::Bool(_) | Value::String(_) => return None,
        })
    }

    /// Any integer value as `i64`, if it fits.
    pub fn as_i64(&self) -> Option<i64> {
        Some(match *self {
            Value::I8(v) => v as i64,
            Value::U8(v) => v as i64,
            Value::I16(v) => v as i64,
            Value::U16(v) => v as i64,
            Value::I32(v) => v as i64,
            Value::U32(v) => v as i64,
            Value::I64(v) => v,
            Value::U64(v) => i64::try_from(v).ok()?,
            _ => return None,
        })
    }
}

/// Formats the value the way the XML encoding writes it (`bool` as `0`/`1`).
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(v) => write!(f, "{}", u8::from(*v)),
            Value::I8(v) => write!(f, "{v}"),
            Value::U8(v) => write!(f, "{v}"),
            Value::I16(v) => write!(f, "{v}"),
            Value::U16(v) => write!(f, "{v}"),
            Value::I32(v) => write!(f, "{v}"),
            Value::U32(v) => write!(f, "{v}"),
            Value::I64(v) => write!(f, "{v}"),
            Value::U64(v) => write!(f, "{v}"),
            Value::F32(v) => write!(f, "{v}"),
            Value::F64(v) => write!(f, "{v}"),
            Value::String(v) => f.write_str(v),
        }
    }
}
