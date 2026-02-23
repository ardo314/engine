mod api;

use api::Api;
use clap::Parser;
use engine_ecs::World;
use engine_schema::Schema;
use std::path::PathBuf;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "engine-server", about = "Distributed ECS server over NATS")]
struct Args {
    /// Path to directory containing .ecs schema files
    #[arg(short, long, default_value = "schema")]
    schema_dir: PathBuf,

    /// NATS server URL
    #[arg(short, long, default_value = "nats://127.0.0.1:4222")]
    nats_url: String,

    /// NATS subject prefix
    #[arg(short, long, default_value = "ecs")]
    prefix: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    // Load schema
    info!(dir = %args.schema_dir.display(), "loading schema files");
    let schema = load_schema(&args.schema_dir)?;
    info!(
        records = schema.records.len(),
        systems = schema.systems.len(),
        phases = schema.phases.len(),
        "schema loaded"
    );

    // Validate schema
    if let Err(e) = schema.validate() {
        error!(%e, "schema validation failed");
        // Continue anyway — validation is advisory for cross-file references
    }

    // Create world
    let world = World::new(schema);

    // Connect to NATS
    info!(url = %args.nats_url, "connecting to NATS");
    let client = async_nats::connect(&args.nats_url).await?;
    info!("connected to NATS");

    // Start API
    let mut api = Api::new(world, client, args.prefix);
    api.run().await?;

    Ok(())
}

fn load_schema(dir: &PathBuf) -> Result<Schema, Box<dyn std::error::Error>> {
    let mut schema = Schema::new();

    if !dir.exists() {
        return Err(format!("schema directory not found: {}", dir.display()).into());
    }

    load_schema_recursive(&mut schema, dir)?;

    if schema.records.is_empty() && schema.systems.is_empty() {
        tracing::warn!("no records or systems found in schema files");
    }

    Ok(schema)
}

fn load_schema_recursive(
    schema: &mut Schema,
    dir: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            load_schema_recursive(schema, &path)?;
        } else if path.extension().map(|e| e == "ecs").unwrap_or(false) {
            info!(file = %path.display(), "loading schema file");
            schema.load_file(&path)?;
        }
    }
    Ok(())
}
