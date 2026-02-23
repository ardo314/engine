/// Abstract syntax tree types for the ECS IDL.
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Top-level file
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    pub package: PackageDecl,
    pub imports: Vec<Import>,
    pub items: Vec<TopLevelItem>,
}

// ---------------------------------------------------------------------------
// Package & imports
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageDecl {
    pub namespace: String,
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub package: PackageRef,
    pub items: Vec<ImportItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageRef {
    pub namespace: String,
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportItem {
    pub name: String,
    pub alias: Option<String>,
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TopLevelItem {
    TypeAlias(TypeAlias),
    Enum(EnumDef),
    Variant(VariantDef),
    Flags(FlagsDef),
    Record(RecordDef),
    System(SystemDef),
    Phase(PhaseDef),
    World(WorldDef),
}

// ---------------------------------------------------------------------------
// Type expressions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TypeExpr {
    /// A primitive: bool, u8, u16, u32, u64, i8, i16, i32, i64, f32, f64, string, bytes
    Primitive(String),
    /// A named type (record, enum, variant, flags, or alias)
    Named(String),
    /// list<T>
    List(Box<TypeExpr>),
    /// option<T>
    Option(Box<TypeExpr>),
    /// set<T>
    Set(Box<TypeExpr>),
    /// map<K, V>
    Map(Box<TypeExpr>, Box<TypeExpr>),
    /// tuple<T1, T2, ...>
    Tuple(Vec<TypeExpr>),
}

// ---------------------------------------------------------------------------
// Type alias
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeAlias {
    pub name: String,
    pub target: TypeExpr,
}

// ---------------------------------------------------------------------------
// Enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<String>,
}

// ---------------------------------------------------------------------------
// Variant (tagged union)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantDef {
    pub name: String,
    pub cases: Vec<VariantCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantCase {
    pub name: String,
    pub payload: Option<Vec<TypeExpr>>,
}

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagsDef {
    pub name: String,
    pub flags: Vec<String>,
}

// ---------------------------------------------------------------------------
// Record (component / tag / event)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordDef {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub ty: TypeExpr,
}

impl RecordDef {
    /// An empty record is a zero-sized tag.
    pub fn is_tag(&self) -> bool {
        self.fields.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Phase
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseDef {
    pub name: String,
    pub hz: Option<u32>,
}

// ---------------------------------------------------------------------------
// System
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemDef {
    pub name: String,
    pub queries: Vec<QueryDef>,
    pub phase: Option<String>,
    pub order_after: Vec<String>,
    pub order_before: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryDef {
    pub name: Option<String>,
    pub read: Vec<String>,
    pub write: Vec<String>,
    pub optional: Vec<String>,
    pub exclude: Vec<String>,
    pub changed: Vec<String>,
}

// ---------------------------------------------------------------------------
// World
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldDef {
    pub name: String,
    pub includes: Vec<IncludeStmt>,
    pub items: Vec<TopLevelItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncludeStmt {
    pub package: PackageRef,
    pub item: Option<String>,
}
