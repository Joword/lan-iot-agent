//! Shared MongoDB client for Hub (auth + scenes + companions).
//!
//! One connection / database (`MONGODB_URI` / `lan_iot` by default). Collections:
//! `pairing_codes`, `tokens`, `scenes`, `companions`.

use mongodb::bson::doc;
use mongodb::options::ClientOptions;
use mongodb::{Client, Collection, Database};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const SERVER_SELECTION_TIMEOUT: Duration = Duration::from_secs(5);

/// Shared MongoDB handle. Cheap to clone (shares the underlying client pool).
#[derive(Clone)]
pub struct MongoStore {
    client: Client,
    database: String,
}

impl MongoStore {
    /// Parse URI, ping the server, and return a store. Fails fast on unreachable Mongo.
    pub async fn connect(uri: &str, database: &str) -> Result<Self, mongodb::error::Error> {
        let mut opts = ClientOptions::parse(uri).await?;
        opts.connect_timeout = Some(CONNECT_TIMEOUT);
        opts.server_selection_timeout = Some(SERVER_SELECTION_TIMEOUT);
        let client = Client::with_options(opts)?;
        let db = client.database(database);
        db.run_command(doc! { "ping": 1 }).await?;
        Ok(Self {
            client,
            database: database.to_string(),
        })
    }

    /// Connect or log and return `None` so Hub can still boot.
    pub async fn connect_optional(uri: &str, database: &str) -> Option<Self> {
        match Self::connect(uri, database).await {
            Ok(store) => {
                tracing::info!(
                    uri = %redact_uri(uri),
                    database,
                    "MongoDB connected"
                );
                Some(store)
            }
            Err(e) => {
                tracing::error!(
                    uri = %redact_uri(uri),
                    database,
                    error = %e,
                    "MongoDB unavailable — auth/scenes/companions persistence disabled"
                );
                None
            }
        }
    }

    pub fn database_name(&self) -> &str {
        &self.database
    }

    pub fn database(&self) -> Database {
        self.client.database(&self.database)
    }

    pub fn collection<T: Send + Sync>(&self, name: &str) -> Collection<T> {
        self.database().collection(name)
    }

    /// Live connectivity check (`ping` command against the configured database).
    pub async fn ping(&self) -> Result<(), mongodb::error::Error> {
        self.database().run_command(doc! { "ping": 1 }).await?;
        Ok(())
    }
}

/// Avoid logging credentials if present in the URI.
pub fn redact_uri(uri: &str) -> String {
    if let Some(at) = uri.rfind('@') {
        if let Some(scheme_end) = uri.find("://") {
            return format!("{}://***@{}", &uri[..scheme_end], &uri[at + 1..]);
        }
    }
    uri.to_string()
}
