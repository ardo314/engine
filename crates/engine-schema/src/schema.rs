/// Resolved schema registry — collects all definitions from parsed files into
/// a unified type registry that the ECS runtime uses for validation.
use crate::ast::*;
use crate::parser::Parser;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("parse error: {0}")]
    Parse(#[from] crate::parser::ParseError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("duplicate type: {0}")]
    DuplicateType(String),
    #[error("duplicate system: {0}")]
    DuplicateSystem(String),
    #[error("duplicate phase: {0}")]
    DuplicatePhase(String),
    #[error("unknown type referenced: {0}")]
    UnknownType(String),
}

/// A resolved schema containing all definitions.
#[derive(Debug, Clone, Default)]
pub struct Schema {
    /// All record definitions (components/tags/events), keyed by name.
    pub records: HashMap<String, RecordDef>,
    /// All enum definitions, keyed by name.
    pub enums: HashMap<String, EnumDef>,
    /// All variant definitions, keyed by name.
    pub variants: HashMap<String, VariantDef>,
    /// All flags definitions, keyed by name.
    pub flags: HashMap<String, FlagsDef>,
    /// All type aliases, keyed by name.
    pub type_aliases: HashMap<String, TypeAlias>,
    /// All system definitions, keyed by name.
    pub systems: HashMap<String, SystemDef>,
    /// All phase definitions, keyed by name.
    pub phases: HashMap<String, PhaseDef>,
    /// Loaded packages (namespace:name -> version).
    pub packages: HashMap<String, Option<String>>,
}

impl Schema {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a `.ecs` file and merge its definitions into this schema.
    pub fn load_file(&mut self, path: &Path) -> Result<(), SchemaError> {
        let source = std::fs::read_to_string(path)?;
        self.load_source(&source)
    }

    /// Parse a source string and merge its definitions.
    pub fn load_source(&mut self, source: &str) -> Result<(), SchemaError> {
        let file = Parser::parse(source)?;
        let pkg_key = format!("{}:{}", file.package.namespace, file.package.name);
        self.packages.insert(pkg_key, file.package.version);
        self.merge_items(&file.items)?;
        Ok(())
    }

    fn merge_items(&mut self, items: &[TopLevelItem]) -> Result<(), SchemaError> {
        for item in items {
            match item {
                TopLevelItem::TypeAlias(ta) => {
                    self.type_aliases
                        .entry(ta.name.clone())
                        .or_insert_with(|| ta.clone());
                }
                TopLevelItem::Enum(e) => {
                    self.enums
                        .entry(e.name.clone())
                        .or_insert_with(|| e.clone());
                }
                TopLevelItem::Variant(v) => {
                    self.variants
                        .entry(v.name.clone())
                        .or_insert_with(|| v.clone());
                }
                TopLevelItem::Flags(f) => {
                    self.flags
                        .entry(f.name.clone())
                        .or_insert_with(|| f.clone());
                }
                TopLevelItem::Record(r) => {
                    // Skip duplicate records — they may come from multiple
                    // domain files (e.g. physics defines `transform` and
                    // gameplay imports it, but both files are loaded).
                    self.records
                        .entry(r.name.clone())
                        .or_insert_with(|| r.clone());
                }
                TopLevelItem::System(s) => {
                    self.systems
                        .entry(s.name.clone())
                        .or_insert_with(|| s.clone());
                }
                TopLevelItem::Phase(p) => {
                    // Allow re-declaration of phases with the same name (common
                    // when multiple domain files share phases like fixed_update).
                    self.phases.insert(p.name.clone(), p.clone());
                }
                TopLevelItem::World(w) => {
                    // Flatten world items into the schema
                    self.merge_items(&w.items)?;
                }
            }
        }
        Ok(())
    }

    /// Get a record definition by name, or None if it doesn't exist.
    pub fn get_record(&self, name: &str) -> Option<&RecordDef> {
        self.records.get(name)
    }

    /// Check if a name refers to any known type (record, enum, variant, flags, alias, or primitive).
    pub fn is_known_type(&self, name: &str) -> bool {
        is_primitive(name)
            || self.records.contains_key(name)
            || self.enums.contains_key(name)
            || self.variants.contains_key(name)
            || self.flags.contains_key(name)
            || self.type_aliases.contains_key(name)
    }

    /// List all record names (components, tags, events).
    pub fn record_names(&self) -> Vec<&str> {
        self.records.keys().map(|s| s.as_str()).collect()
    }

    /// List all tag record names (empty records).
    pub fn tag_names(&self) -> Vec<&str> {
        self.records
            .iter()
            .filter(|(_, r)| r.is_tag())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    /// List all component record names (non-empty records).
    pub fn component_names(&self) -> Vec<&str> {
        self.records
            .iter()
            .filter(|(_, r)| !r.is_tag())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    /// Validate that all types referenced in records and systems are defined.
    pub fn validate(&self) -> Result<(), SchemaError> {
        // Validate record field types
        for rec in self.records.values() {
            for field in &rec.fields {
                self.validate_type_expr(&field.ty)?;
            }
        }

        // Validate system query references point to known records
        for sys in self.systems.values() {
            for query in &sys.queries {
                for name in query
                    .read
                    .iter()
                    .chain(&query.write)
                    .chain(&query.optional)
                    .chain(&query.exclude)
                    .chain(&query.changed)
                {
                    if !self.records.contains_key(name) {
                        return Err(SchemaError::UnknownType(format!(
                            "system '{}' references unknown record '{}'",
                            sys.name, name
                        )));
                    }
                }
            }

            // Validate phase references
            if let Some(ref phase) = sys.phase {
                if !self.phases.contains_key(phase) {
                    return Err(SchemaError::UnknownType(format!(
                        "system '{}' references unknown phase '{}'",
                        sys.name, phase
                    )));
                }
            }
        }

        Ok(())
    }

    fn validate_type_expr(&self, ty: &TypeExpr) -> Result<(), SchemaError> {
        match ty {
            TypeExpr::Primitive(_) => Ok(()),
            TypeExpr::Named(name) => {
                if self.is_known_type(name) {
                    Ok(())
                } else {
                    Err(SchemaError::UnknownType(name.clone()))
                }
            }
            TypeExpr::List(inner) | TypeExpr::Option(inner) | TypeExpr::Set(inner) => {
                self.validate_type_expr(inner)
            }
            TypeExpr::Map(k, v) => {
                self.validate_type_expr(k)?;
                self.validate_type_expr(v)
            }
            TypeExpr::Tuple(types) => {
                for t in types {
                    self.validate_type_expr(t)?;
                }
                Ok(())
            }
        }
    }

    /// Serialize the schema to a JSON description for clients.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "records": self.records.values().map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "is_tag": r.is_tag(),
                    "fields": r.fields.iter().map(|f| {
                        serde_json::json!({
                            "name": f.name,
                            "type": format!("{:?}", f.ty),
                        })
                    }).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
            "systems": self.systems.values().map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "phase": s.phase,
                    "queries": s.queries.iter().map(|q| {
                        serde_json::json!({
                            "name": q.name,
                            "read": q.read,
                            "write": q.write,
                            "optional": q.optional,
                            "exclude": q.exclude,
                            "changed": q.changed,
                        })
                    }).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
            "phases": self.phases.values().map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "hz": p.hz,
                })
            }).collect::<Vec<_>>(),
            "packages": self.packages,
        })
    }
}

fn is_primitive(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "f32"
            | "f64"
            | "string"
            | "bytes"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_and_validate() {
        let mut schema = Schema::new();
        schema
            .load_source(
                r#"
            package test:game@0.1.0

            record transform {
                x: f32,
                y: f32,
                z: f32,
            }

            record velocity {
                x: f32,
                y: f32,
                z: f32,
            }

            record frozen {}

            phase fixed_update { hz: 60 }

            system movement {
                query {
                    read: [velocity],
                    write: [transform],
                    exclude: [frozen],
                }
                phase: fixed_update,
            }
        "#,
            )
            .unwrap();

        schema.validate().unwrap();
        assert_eq!(schema.records.len(), 3);
        assert_eq!(schema.systems.len(), 1);
        assert!(schema.get_record("frozen").unwrap().is_tag());
    }
}
