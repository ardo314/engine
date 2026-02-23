/// ECS World — runtime entity-component storage with dynamic typing.
///
/// Components are schema-defined (not Rust types), so we store them as
/// `serde_json::Value` keyed by component name. The schema is used at
/// runtime to validate incoming data.
use engine_schema::{Schema, TypeExpr};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use thiserror::Error;

pub type EntityId = u64;

#[derive(Debug, Error)]
pub enum WorldError {
    #[error("entity {0} not found")]
    EntityNotFound(EntityId),
    #[error("unknown record: {0}")]
    UnknownRecord(String),
    #[error("validation error on '{component}': {message}")]
    ValidationError {
        component: String,
        message: String,
    },
    #[error("component '{0}' not found on entity {1}")]
    ComponentNotFound(String, EntityId),
}

/// A single entity's component set.
#[derive(Debug, Clone, Default)]
struct EntityData {
    components: HashMap<String, Value>,
}

/// The ECS world: entity storage, schema-validated component operations.
pub struct World {
    schema: Schema,
    next_entity: EntityId,
    entities: HashMap<EntityId, EntityData>,
    /// Tracks which entities had a component changed this tick (for `changed` queries).
    changed: HashMap<String, HashSet<EntityId>>,
}

impl World {
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            next_entity: 1,
            entities: HashMap::new(),
            changed: HashMap::new(),
        }
    }

    /// Access the schema.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    // -- Entity lifecycle --

    /// Spawn a new entity, optionally with initial components.
    pub fn spawn(
        &mut self,
        components: Option<HashMap<String, Value>>,
    ) -> Result<EntityId, WorldError> {
        let id = self.next_entity;
        self.next_entity += 1;

        let mut data = EntityData::default();

        if let Some(comps) = components {
            for (name, value) in comps {
                self.validate_component(&name, &value)?;
                self.mark_changed(&name, id);
                data.components.insert(name, value);
            }
        }

        self.entities.insert(id, data);
        Ok(id)
    }

    /// Despawn an entity, removing all its components.
    pub fn despawn(&mut self, id: EntityId) -> Result<(), WorldError> {
        if self.entities.remove(&id).is_none() {
            return Err(WorldError::EntityNotFound(id));
        }
        // Clean up change tracking
        for set in self.changed.values_mut() {
            set.remove(&id);
        }
        Ok(())
    }

    /// Check if an entity exists.
    pub fn exists(&self, id: EntityId) -> bool {
        self.entities.contains_key(&id)
    }

    /// Return all entity IDs.
    pub fn all_entities(&self) -> Vec<EntityId> {
        self.entities.keys().copied().collect()
    }

    /// Return the count of live entities.
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    // -- Component operations --

    /// Set a component on an entity. Validates against schema.
    pub fn set_component(
        &mut self,
        id: EntityId,
        component: &str,
        value: Value,
    ) -> Result<(), WorldError> {
        self.validate_component(component, &value)?;
        let data = self
            .entities
            .get_mut(&id)
            .ok_or(WorldError::EntityNotFound(id))?;
        data.components.insert(component.to_string(), value);
        self.mark_changed(component, id);
        Ok(())
    }

    /// Get a component value from an entity.
    pub fn get_component(&self, id: EntityId, component: &str) -> Result<&Value, WorldError> {
        let data = self
            .entities
            .get(&id)
            .ok_or(WorldError::EntityNotFound(id))?;
        data.components
            .get(component)
            .ok_or_else(|| WorldError::ComponentNotFound(component.to_string(), id))
    }

    /// Remove a component from an entity.
    pub fn remove_component(&mut self, id: EntityId, component: &str) -> Result<(), WorldError> {
        let data = self
            .entities
            .get_mut(&id)
            .ok_or(WorldError::EntityNotFound(id))?;
        if data.components.remove(component).is_none() {
            return Err(WorldError::ComponentNotFound(component.to_string(), id));
        }
        Ok(())
    }

    /// Check if an entity has a specific component.
    pub fn has_component(&self, id: EntityId, component: &str) -> bool {
        self.entities
            .get(&id)
            .map(|d| d.components.contains_key(component))
            .unwrap_or(false)
    }

    /// Get all component names on an entity.
    pub fn entity_components(&self, id: EntityId) -> Result<Vec<String>, WorldError> {
        let data = self
            .entities
            .get(&id)
            .ok_or(WorldError::EntityNotFound(id))?;
        Ok(data.components.keys().cloned().collect())
    }

    /// Get all components on an entity as a map.
    pub fn entity_snapshot(&self, id: EntityId) -> Result<&HashMap<String, Value>, WorldError> {
        let data = self
            .entities
            .get(&id)
            .ok_or(WorldError::EntityNotFound(id))?;
        Ok(&data.components)
    }

    // -- Query --

    /// Query entities matching a filter. Returns entity IDs that match.
    ///
    ///  - `with`: entity must have ALL of these components
    ///  - `without`: entity must have NONE of these components
    ///  - `changed_components`: if non-empty, at least one must be in the changed set
    pub fn query(
        &self,
        with: &[String],
        without: &[String],
        changed_components: &[String],
    ) -> Vec<EntityId> {
        self.entities
            .iter()
            .filter(|(id, data)| {
                // Must have all `with` components
                let has_all = with.iter().all(|c| data.components.contains_key(c));
                if !has_all {
                    return false;
                }

                // Must not have any `without` components
                let has_none = without.iter().all(|c| !data.components.contains_key(c));
                if !has_none {
                    return false;
                }

                // If changed filter is specified, at least one must have changed
                if !changed_components.is_empty() {
                    let any_changed = changed_components.iter().any(|c| {
                        self.changed
                            .get(c)
                            .map(|set| set.contains(id))
                            .unwrap_or(false)
                    });
                    if !any_changed {
                        return false;
                    }
                }

                true
            })
            .map(|(id, _)| *id)
            .collect()
    }

    // -- Change tracking --

    fn mark_changed(&mut self, component: &str, entity: EntityId) {
        self.changed
            .entry(component.to_string())
            .or_default()
            .insert(entity);
    }

    /// Clear all change tracking. Call this at the end of each tick.
    pub fn clear_changes(&mut self) {
        self.changed.clear();
    }

    /// Get entities that had a specific component changed.
    pub fn get_changed(&self, component: &str) -> HashSet<EntityId> {
        self.changed
            .get(component)
            .cloned()
            .unwrap_or_default()
    }

    // -- Validation --

    /// Validate a component value against the schema.
    fn validate_component(&self, name: &str, value: &Value) -> Result<(), WorldError> {
        let record = self
            .schema
            .get_record(name)
            .ok_or_else(|| WorldError::UnknownRecord(name.to_string()))?;

        // Tag (empty record) — must be null or empty object
        if record.is_tag() {
            match value {
                Value::Null => return Ok(()),
                Value::Object(_) => return Ok(()),
                _ => {
                    return Err(WorldError::ValidationError {
                        component: name.to_string(),
                        message: "tag component must be null or empty object".to_string(),
                    })
                }
            }
        }

        // Component with fields — expect a JSON object
        let obj = value.as_object().ok_or_else(|| WorldError::ValidationError {
            component: name.to_string(),
            message: "expected JSON object".to_string(),
        })?;

        // Check required fields are present
        for field in &record.fields {
            if !obj.contains_key(&field.name) {
                // Allow missing fields if the type is option<T>
                if matches!(field.ty, TypeExpr::Option(_)) {
                    continue;
                }
                return Err(WorldError::ValidationError {
                    component: name.to_string(),
                    message: format!("missing required field '{}'", field.name),
                });
            }
        }

        // Type validation for each provided field
        for field in &record.fields {
            if let Some(val) = obj.get(&field.name) {
                self.validate_value(val, &field.ty, name, &field.name)?;
            }
        }

        Ok(())
    }

    fn validate_value(
        &self,
        value: &Value,
        ty: &TypeExpr,
        component: &str,
        field: &str,
    ) -> Result<(), WorldError> {
        let err = |msg: String| WorldError::ValidationError {
            component: component.to_string(),
            message: format!("field '{}': {}", field, msg),
        };

        match ty {
            TypeExpr::Primitive(p) => match p.as_str() {
                "bool" => {
                    value.as_bool().ok_or_else(|| err("expected bool".into()))?;
                }
                "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" => {
                    value
                        .as_number()
                        .ok_or_else(|| err("expected number".into()))?;
                }
                "f32" | "f64" => {
                    value
                        .as_number()
                        .ok_or_else(|| err("expected number".into()))?;
                }
                "string" => {
                    value
                        .as_str()
                        .ok_or_else(|| err("expected string".into()))?;
                }
                "bytes" => {
                    // Accept string (base64) or array of numbers
                    if !value.is_string() && !value.is_array() {
                        return Err(err("expected string or array for bytes".into()));
                    }
                }
                _ => {}
            },
            TypeExpr::Named(_) => {
                // Named types (other records, enums, etc.) — accept any valid JSON for now.
                // Full recursive validation could resolve aliases, but that's a future enhancement.
            }
            TypeExpr::List(_inner) => {
                value
                    .as_array()
                    .ok_or_else(|| err("expected array".into()))?;
            }
            TypeExpr::Option(inner) => {
                if !value.is_null() {
                    self.validate_value(value, inner, component, field)?;
                }
            }
            TypeExpr::Set(_) => {
                value
                    .as_array()
                    .ok_or_else(|| err("expected array for set".into()))?;
            }
            TypeExpr::Map(_, _) => {
                value
                    .as_object()
                    .ok_or_else(|| err("expected object for map".into()))?;
            }
            TypeExpr::Tuple(types) => {
                let arr = value
                    .as_array()
                    .ok_or_else(|| err("expected array for tuple".into()))?;
                if arr.len() != types.len() {
                    return Err(err(format!(
                        "tuple has {} elements, expected {}",
                        arr.len(),
                        types.len()
                    )));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_world() -> World {
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
            }

            record frozen {}
            record player {}

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
        World::new(schema)
    }

    #[test]
    fn test_spawn_and_get() {
        let mut world = make_test_world();
        let mut comps = HashMap::new();
        comps.insert(
            "transform".to_string(),
            serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0}),
        );
        let id = world.spawn(Some(comps)).unwrap();
        assert!(world.exists(id));
        let t = world.get_component(id, "transform").unwrap();
        assert_eq!(t["x"], 1.0);
    }

    #[test]
    fn test_tag_component() {
        let mut world = make_test_world();
        let id = world.spawn(None).unwrap();
        world
            .set_component(id, "frozen", Value::Null)
            .unwrap();
        assert!(world.has_component(id, "frozen"));
    }

    #[test]
    fn test_query() {
        let mut world = make_test_world();

        let e1 = world.spawn(None).unwrap();
        world
            .set_component(e1, "transform", serde_json::json!({"x": 0, "y": 0, "z": 0}))
            .unwrap();
        world
            .set_component(e1, "velocity", serde_json::json!({"x": 1, "y": 0}))
            .unwrap();

        let e2 = world.spawn(None).unwrap();
        world
            .set_component(e2, "transform", serde_json::json!({"x": 0, "y": 0, "z": 0}))
            .unwrap();
        world
            .set_component(e2, "velocity", serde_json::json!({"x": 0, "y": 1}))
            .unwrap();
        world
            .set_component(e2, "frozen", Value::Null)
            .unwrap();

        // Query: has transform + velocity, not frozen
        let results = world.query(
            &["transform".into(), "velocity".into()],
            &["frozen".into()],
            &[],
        );
        assert_eq!(results, vec![e1]);
    }

    #[test]
    fn test_change_tracking() {
        let mut world = make_test_world();
        let id = world.spawn(None).unwrap();
        world
            .set_component(id, "transform", serde_json::json!({"x": 0, "y": 0, "z": 0}))
            .unwrap();

        assert!(world.get_changed("transform").contains(&id));
        world.clear_changes();
        assert!(!world.get_changed("transform").contains(&id));
    }

    #[test]
    fn test_validation_rejects_bad_data() {
        let mut world = make_test_world();
        let id = world.spawn(None).unwrap();

        // Missing required field
        let result = world.set_component(id, "transform", serde_json::json!({"x": 1.0}));
        assert!(result.is_err());

        // Unknown component
        let result = world.set_component(id, "nonexistent", serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn test_despawn() {
        let mut world = make_test_world();
        let id = world.spawn(None).unwrap();
        assert!(world.exists(id));
        world.despawn(id).unwrap();
        assert!(!world.exists(id));
    }
}
