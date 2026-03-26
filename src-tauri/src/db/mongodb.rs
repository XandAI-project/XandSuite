use anyhow::{Context, Result};
use mongodb::{Client, options::ClientOptions};
use serde_json::Value;
use std::time::Duration;

use crate::models::QueryResult;

pub struct MongoConnector {
    client: Client,
}

impl MongoConnector {
    pub async fn connect(connection_string: &str) -> Result<Self> {
        let mut options = ClientOptions::parse(connection_string)
            .await
            .context("Failed to parse MongoDB connection string")?;

        options.connect_timeout = Some(Duration::from_secs(10));
        options.server_selection_timeout = Some(Duration::from_secs(10));

        let client = Client::with_options(options)
            .context("Failed to create MongoDB client")?;

        // Ping to verify connection
        client
            .database("admin")
            .run_command(mongodb::bson::doc! { "ping": 1 })
            .await
            .context("Failed to connect to MongoDB - ping failed")?;

        Ok(Self { client })
    }

    pub async fn list_databases(&self) -> Result<Vec<String>> {
        let names = self.client
            .list_database_names()
            .await
            .context("Failed to list databases")?;
        Ok(names)
    }

    pub async fn list_collections(&self, database: &str) -> Result<Vec<String>> {
        let db = self.client.database(database);
        let names = db
            .list_collection_names()
            .await
            .context("Failed to list collections")?;
        Ok(names)
    }

    pub async fn run_query(
        &self,
        database: &str,
        collection: &str,
        filter_json: &str,
        limit: u32,
    ) -> Result<QueryResult> {
        use futures::TryStreamExt;
        use mongodb::options::FindOptions;

        let start = std::time::Instant::now();

        let filter: mongodb::bson::Document = serde_json::from_str::<Value>(filter_json)
            .ok()
            .and_then(|v| mongodb::bson::to_document(&v).ok())
            .unwrap_or_default();

        let options = FindOptions::builder()
            .limit(Some(limit as i64))
            .build();

        let collection = self.client
            .database(database)
            .collection::<mongodb::bson::Document>(collection);

        let mut cursor = collection
            .find(filter)
            .with_options(options)
            .await
            .context("Failed to execute MongoDB query")?;

        let mut rows: Vec<Value> = Vec::new();
        while let Some(doc) = cursor.try_next().await? {
            let json = serde_json::to_value(doc)?;
            rows.push(json);
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        let row_count = rows.len();

        Ok(QueryResult {
            columns: vec![],
            rows,
            row_count,
            duration_ms,
        })
    }
}
