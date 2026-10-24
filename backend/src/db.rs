use redis::{AsyncCommands, RedisResult};
use serde::{Deserialize, Serialize};

/// A multiplexed Redis connection. It is cheap to clone and pipelines
/// commands from many concurrent tasks over one socket, so no lock is needed.
pub type Database = redis::aio::MultiplexedConnection;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Data {
    pub creation_data: String,
    pub shortened_url: String,
    pub long_url: String,
    pub ttl: u32,
    
}


/// Create a new Redis database connection
pub async fn init_db() -> Database {
    let mut try_urls = vec![
        "redis://redis:6379/".to_string(),     // Docker Compose service name
        "redis://127.0.0.1:6379/".to_string(), // Local fallback
    ];
    if let Ok(url) = std::env::var("REDIS_URL") {
        try_urls.insert(0, url);
    }

    for url in try_urls {
        match redis::Client::open(url.as_str()) {
            Ok(client) => match client.get_multiplexed_async_connection().await {
                Ok(conn) => {
                    println!("✅ Connected to Redis at: {}", url);
                    return conn;
                }
                Err(e) => {
                    eprintln!("❌ Failed to connect using {}: {}", url, e);
                }
            },
            Err(e) => {
                eprintln!("❌ Failed to create client using {}: {}", url, e);
            }
        }
    }

    panic!("🚨 Could not connect to Redis on any known address");
}

/// Store data in Redis asynchronously
pub async fn store_data(database: Database, short_url_id: String, data: Data) -> RedisResult<()> {
    let serialized_data = serde_json::to_string(&data).map_err(|e| {
        redis::RedisError::from((
            redis::ErrorKind::TypeError,
            "Serialization error",
            e.to_string(),
        ))
    })?;

    let mut conn = database;
    let _: () = conn
        .set_ex(&short_url_id, serialized_data, data.ttl.into())
        .await?;
    Ok(())
}

/// Retrieve data from Redis asynchronously
pub async fn retrieve_data(database: Database, short_url_id: &str) -> Option<Data> {
    let mut conn = database;
    let serialized_data: String = conn.get(short_url_id).await.ok()?;
    serde_json::from_str(&serialized_data).ok()
}

// Delete expired or invalid data
pub async fn delete_data(database: Database, short_url_id: &str) -> RedisResult<()> {
    let mut conn = database;
    let _:() = conn.del(short_url_id).await?;
    Ok(())
}

