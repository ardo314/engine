/// NATS API handler — subscribes to subjects and handles ECS operations.
///
/// Subjects (all under configurable prefix, default "ecs"):
///
///   Request/Reply:
///     {prefix}.spawn           — create entity, optional initial components
///     {prefix}.despawn         — destroy entity
///     {prefix}.set             — set component on entity
///     {prefix}.get             — get component from entity
///     {prefix}.remove          — remove component from entity
///     {prefix}.query           — query entities by component filters
///     {prefix}.entity          — get full entity snapshot
///     {prefix}.entities        — list all entity IDs
///     {prefix}.schema          — get schema info
///     {prefix}.schema.record   — get record schema by name
///
///   Publish (broadcast):
///     {prefix}.events.spawned      — entity spawned
///     {prefix}.events.despawned    — entity despawned
///     {prefix}.events.changed.{component} — component changed
use async_nats::Client;
use engine_ecs::{EntityId, World};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, error, info, warn};

pub struct Api {
    world: World,
    client: Client,
    prefix: String,
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SpawnRequest {
    #[serde(default)]
    components: Option<HashMap<String, Value>>,
}

#[derive(Serialize)]
struct SpawnResponse {
    entity_id: EntityId,
}

#[derive(Deserialize)]
struct EntityRequest {
    entity_id: EntityId,
}

#[derive(Deserialize)]
struct SetComponentRequest {
    entity_id: EntityId,
    component: String,
    value: Value,
}

#[derive(Deserialize)]
struct GetComponentRequest {
    entity_id: EntityId,
    component: String,
}

#[derive(Deserialize)]
struct RemoveComponentRequest {
    entity_id: EntityId,
    component: String,
}

#[derive(Deserialize)]
struct QueryRequest {
    #[serde(default)]
    with: Vec<String>,
    #[serde(default)]
    without: Vec<String>,
    #[serde(default)]
    changed: Vec<String>,
}

#[derive(Deserialize)]
struct SchemaRecordRequest {
    name: String,
}

#[derive(Serialize)]
struct ApiResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    ok: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl ApiResponse {
    fn ok(value: Value) -> Self {
        Self {
            ok: Some(value),
            error: None,
        }
    }

    fn error(msg: impl Into<String>) -> Self {
        Self {
            ok: None,
            error: Some(msg.into()),
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| b"{}".to_vec())
    }
}

// ---------------------------------------------------------------------------
// Event payloads (broadcast)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct EntityEvent {
    entity_id: EntityId,
}

#[derive(Serialize)]
struct ComponentChangedEvent {
    entity_id: EntityId,
    component: String,
    value: Value,
}

// ---------------------------------------------------------------------------
// API implementation
// ---------------------------------------------------------------------------

impl Api {
    pub fn new(world: World, client: Client, prefix: String) -> Self {
        Self {
            world,
            client,
            prefix,
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        use futures_util::StreamExt;

        // Subscribe to all subjects under prefix using wildcard
        let subject = format!("{}.>", self.prefix);
        info!(subject = %subject, "subscribing to API subjects");
        let mut sub = self.client.subscribe(subject).await?;

        info!("engine-server ready — listening for requests");

        while let Some(msg) = sub.next().await {
            let subject = msg.subject.as_str().to_string();
            let reply = msg.reply.clone();

            // Strip prefix to get the operation
            let op = subject
                .strip_prefix(&self.prefix)
                .and_then(|s| s.strip_prefix('.'))
                .unwrap_or("");

            debug!(op = %op, "received request");

            let response = match op {
                "spawn" => self.handle_spawn(&msg.payload).await,
                "despawn" => self.handle_despawn(&msg.payload).await,
                "set" => self.handle_set(&msg.payload).await,
                "get" => self.handle_get(&msg.payload),
                "remove" => self.handle_remove(&msg.payload).await,
                "query" => self.handle_query(&msg.payload),
                "entity" => self.handle_entity(&msg.payload),
                "entities" => self.handle_entities(),
                "schema" => self.handle_schema(),
                s if s.starts_with("schema.record") => {
                    self.handle_schema_record(&msg.payload)
                }
                _ => {
                    warn!(op = %op, "unknown operation");
                    ApiResponse::error(format!("unknown operation: {op}"))
                }
            };

            // Reply if request/reply pattern
            if let Some(reply_to) = reply {
                if let Err(e) = self
                    .client
                    .publish(reply_to, response.to_bytes().into())
                    .await
                {
                    error!(%e, "failed to publish reply");
                }
            }
        }

        Ok(())
    }

    // -- Handlers --

    async fn handle_spawn(&mut self, payload: &[u8]) -> ApiResponse {
        let req: SpawnRequest = match serde_json::from_slice(payload) {
            Ok(r) => r,
            Err(e) => return ApiResponse::error(format!("invalid request: {e}")),
        };

        match self.world.spawn(req.components) {
            Ok(id) => {
                // Broadcast spawn event
                let event = EntityEvent { entity_id: id };
                let subject = format!("{}.events.spawned", self.prefix);
                let _ = self
                    .client
                    .publish(subject, serde_json::to_vec(&event).unwrap().into())
                    .await;

                ApiResponse::ok(serde_json::to_value(SpawnResponse { entity_id: id }).unwrap())
            }
            Err(e) => ApiResponse::error(e.to_string()),
        }
    }

    async fn handle_despawn(&mut self, payload: &[u8]) -> ApiResponse {
        let req: EntityRequest = match serde_json::from_slice(payload) {
            Ok(r) => r,
            Err(e) => return ApiResponse::error(format!("invalid request: {e}")),
        };

        match self.world.despawn(req.entity_id) {
            Ok(()) => {
                // Broadcast despawn event
                let event = EntityEvent {
                    entity_id: req.entity_id,
                };
                let subject = format!("{}.events.despawned", self.prefix);
                let _ = self
                    .client
                    .publish(subject, serde_json::to_vec(&event).unwrap().into())
                    .await;

                ApiResponse::ok(Value::Null)
            }
            Err(e) => ApiResponse::error(e.to_string()),
        }
    }

    async fn handle_set(&mut self, payload: &[u8]) -> ApiResponse {
        let req: SetComponentRequest = match serde_json::from_slice(payload) {
            Ok(r) => r,
            Err(e) => return ApiResponse::error(format!("invalid request: {e}")),
        };

        match self
            .world
            .set_component(req.entity_id, &req.component, req.value.clone())
        {
            Ok(()) => {
                // Broadcast change event
                let event = ComponentChangedEvent {
                    entity_id: req.entity_id,
                    component: req.component.clone(),
                    value: req.value,
                };
                let subject = format!("{}.events.changed.{}", self.prefix, req.component);
                let _ = self
                    .client
                    .publish(subject, serde_json::to_vec(&event).unwrap().into())
                    .await;

                ApiResponse::ok(Value::Null)
            }
            Err(e) => ApiResponse::error(e.to_string()),
        }
    }

    fn handle_get(&self, payload: &[u8]) -> ApiResponse {
        let req: GetComponentRequest = match serde_json::from_slice(payload) {
            Ok(r) => r,
            Err(e) => return ApiResponse::error(format!("invalid request: {e}")),
        };

        match self.world.get_component(req.entity_id, &req.component) {
            Ok(value) => ApiResponse::ok(value.clone()),
            Err(e) => ApiResponse::error(e.to_string()),
        }
    }

    async fn handle_remove(&mut self, payload: &[u8]) -> ApiResponse {
        let req: RemoveComponentRequest = match serde_json::from_slice(payload) {
            Ok(r) => r,
            Err(e) => return ApiResponse::error(format!("invalid request: {e}")),
        };

        match self.world.remove_component(req.entity_id, &req.component) {
            Ok(()) => {
                // Broadcast removal as a change with null value
                let event = ComponentChangedEvent {
                    entity_id: req.entity_id,
                    component: req.component.clone(),
                    value: Value::Null,
                };
                let subject = format!("{}.events.changed.{}", self.prefix, req.component);
                let _ = self
                    .client
                    .publish(subject, serde_json::to_vec(&event).unwrap().into())
                    .await;

                ApiResponse::ok(Value::Null)
            }
            Err(e) => ApiResponse::error(e.to_string()),
        }
    }

    fn handle_query(&self, payload: &[u8]) -> ApiResponse {
        let req: QueryRequest = match serde_json::from_slice(payload) {
            Ok(r) => r,
            Err(e) => return ApiResponse::error(format!("invalid request: {e}")),
        };

        let entities = self.world.query(&req.with, &req.without, &req.changed);
        ApiResponse::ok(serde_json::json!({ "entities": entities }))
    }

    fn handle_entity(&self, payload: &[u8]) -> ApiResponse {
        let req: EntityRequest = match serde_json::from_slice(payload) {
            Ok(r) => r,
            Err(e) => return ApiResponse::error(format!("invalid request: {e}")),
        };

        match self.world.entity_snapshot(req.entity_id) {
            Ok(components) => ApiResponse::ok(serde_json::json!({
                "entity_id": req.entity_id,
                "components": components,
            })),
            Err(e) => ApiResponse::error(e.to_string()),
        }
    }

    fn handle_entities(&self) -> ApiResponse {
        let ids = self.world.all_entities();
        ApiResponse::ok(serde_json::json!({
            "entities": ids,
            "count": ids.len(),
        }))
    }

    fn handle_schema(&self) -> ApiResponse {
        ApiResponse::ok(self.world.schema().to_json())
    }

    fn handle_schema_record(&self, payload: &[u8]) -> ApiResponse {
        let req: SchemaRecordRequest = match serde_json::from_slice(payload) {
            Ok(r) => r,
            Err(e) => return ApiResponse::error(format!("invalid request: {e}")),
        };

        match self.world.schema().get_record(&req.name) {
            Some(record) => ApiResponse::ok(serde_json::json!({
                "name": record.name,
                "is_tag": record.is_tag(),
                "fields": record.fields.iter().map(|f| {
                    serde_json::json!({
                        "name": f.name,
                        "type": format!("{:?}", f.ty),
                    })
                }).collect::<Vec<_>>(),
            })),
            None => ApiResponse::error(format!("unknown record: {}", req.name)),
        }
    }
}
