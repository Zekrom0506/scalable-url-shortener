use crate::db::{retrieve_data, store_data, Data, Database};
use base62;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::Mutex;
use warp::{http::StatusCode, reject, Filter};

// Define a custom error that implements warp::reject::Reject
#[derive(Debug)]
pub struct RedisError(pub String);
impl reject::Reject for RedisError {}

const EPOCH: i64 = 1609459200000; // Custom epoch (e.g., 2021-01-01)
const NODE_ID_BITS: i64 = 10;
const SEQUENCE_BITS: i64 = 12;

const MAX_NODE_ID: i64 = (1 << NODE_ID_BITS) - 1;
const MAX_SEQUENCE: i64 = (1 << SEQUENCE_BITS) - 1;

/// Extract Redis connection from the Arc<Mutex> and pass it into the handler functions.
pub fn with_db(
    db: Database,
) -> impl Filter<Extract = (Database,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || db.clone())
}

const BASE_URL: &str = "http://rustyshortener";

/// Handle the generation of short URLs, storing the information in Redis.
pub async fn handle_generate_url(
    key: String,
    body: serde_json::Value,
    redis_connection: Database,
    api_key: String,
) -> Result<impl warp::Reply, warp::Rejection> {
    // Verify API key authorization
    if key != api_key {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "UNAUTHORIZED" })),
            StatusCode::UNAUTHORIZED,
        ));
    }

    // Validate long URL from request body
    let long_url = body["long_url"].as_str().unwrap_or("");
    if long_url.is_empty() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "INVALID URL" })),
            StatusCode::BAD_REQUEST,
        ));
    }

    // Generate the short URL
    let id = generate_short_url_id(long_url);
    let full = format!("{}/dns_resolver/{}", BASE_URL, id);

    let data = Data {
        creation_data: chrono::Local::now().to_rfc3339(),
        shortened_url: full.clone(),
        long_url: long_url.to_string(),
        ttl: 30,
    };

    store_data(redis_connection, id.clone(), data)
        .await
        .map_err(|e| reject::custom(RedisError(format!("Redis storage error: {}", e))))?;

    let body = serde_json::json!({
      "status": "success",
      "short_url": full
    });
    Ok(warp::reply::with_status(
        warp::reply::json(&body),
        StatusCode::OK,
    ))
}

/// Snowflake ID generator: 41-bit ms timestamp | 10-bit node id | 12-bit sequence.
/// Each backend gets a distinct node id (NODE_ID env var), so IDs are unique
/// across the whole cluster without any coordination.
pub struct SnowflakeGenerator {
    node_id: i64,
    /// (last_timestamp, sequence) updated together under one lock so two
    /// concurrent requests can never be handed the same (timestamp, seq) pair.
    state: Mutex<(i64, i64)>,
}

impl SnowflakeGenerator {
    pub fn new(node_id: i64) -> Self {
        assert!(
            (0..=MAX_NODE_ID).contains(&node_id),
            "Node ID must be between 0 and {}",
            MAX_NODE_ID
        );
        SnowflakeGenerator {
            node_id,
            state: Mutex::new((-1, 0)),
        }
    }

    fn timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Clock moved backwards")
            .as_millis() as i64
            - EPOCH
    }

    pub fn generate(&self) -> i64 {
        let mut state = self.state.lock().unwrap();
        let (last, seq) = *state;
        let mut timestamp = Self::timestamp().max(last); // tolerate clock going backwards

        let seq = if timestamp == last {
            let next = (seq + 1) & MAX_SEQUENCE;
            if next == 0 {
                // 4096 IDs used in this millisecond: wait for the next one.
                while timestamp <= last {
                    std::hint::spin_loop();
                    timestamp = Self::timestamp();
                }
            }
            next
        } else {
            0
        };

        *state = (timestamp, seq);
        (timestamp << (NODE_ID_BITS + SEQUENCE_BITS)) | (self.node_id << SEQUENCE_BITS) | seq
    }
}

static GENERATOR: once_cell::sync::Lazy<SnowflakeGenerator> = once_cell::sync::Lazy::new(|| {
    let node_id = std::env::var("NODE_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    SnowflakeGenerator::new(node_id)
});

/// Generate a unique short URL ID. The full Snowflake ID is base62-encoded
/// (no truncation), so distinct IDs always give distinct short codes.
pub fn generate_short_url_id(_long_url: &str) -> String {
    base62::encode(GENERATOR.generate() as u64)
}

/// Handle redirect for a given short URL.
pub async fn handle_redirect_url(
    params: HashMap<String, String>,
    redis_connection: Database,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let short_url = params.get("short_url").cloned().unwrap_or_default();

    if let Some(data) = retrieve_data(redis_connection, &short_url).await {
        let now = chrono::Local::now();
        let expiration_time = chrono::DateTime::parse_from_rfc3339(&data.creation_data).unwrap()
            + chrono::Duration::seconds(data.ttl.into());

        if now > expiration_time {
            return Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "status": "error",
                    "message": "Short URL has expired!"
                })),
                StatusCode::NOT_FOUND,
            )));
        }

        // Perform HTTP redirect to the long URL
        if let Ok(uri) = data.long_url.parse::<warp::http::Uri>() {
            return Ok(Box::new(warp::redirect::temporary(uri)));
        } else {
            return Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "status": "error",
                    "message": "Invalid long URL format"
                })),
                StatusCode::BAD_REQUEST,
            )));
        }
    }

    // URL not found
    Ok(Box::new(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "status": "error",
            "message": "Short URL not found"
        })),
        StatusCode::NOT_FOUND,
    )))
}

/// Handle creation of a user-defined custom short URL.
pub async fn handle_custom_url(
    key: String,
    body: serde_json::Value,
    redis_connection: Database,
    expected_api_key: String,
) -> Result<impl warp::Reply, warp::Rejection> {
    // Verify API key
    if key != expected_api_key {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "UNAUTHORIZED" })),
            StatusCode::UNAUTHORIZED,
        ));
    }

    // Validate input fields
    let long_url = body["long_url"].as_str().unwrap_or("");
    let custom_short = body["custom_short"].as_str().unwrap_or("");
    if long_url.is_empty() || custom_short.is_empty() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Missing long_url or custom_short"
            })),
            StatusCode::BAD_REQUEST,
        ));
    }

    // Check for existing alias (collision detection)
    if retrieve_data(redis_connection.clone(), custom_short).await.is_some() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "Alias already exists" })),
            StatusCode::CONFLICT,
        ));
    }

    // Build full URL and data struct
    let full = format!("{}/dns_resolver/{}", BASE_URL, custom_short);
    let data = Data {
        creation_data: chrono::Local::now().to_rfc3339(),
        shortened_url: full.clone(),
        long_url: long_url.to_string(),
        ttl: 30,
    };

    // Attempt to store in Redis
    let key = custom_short.to_string();
    match store_data(redis_connection, key.clone(), data).await {
        Ok(_) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "status": "success",
                "short_url": full
            })),
            StatusCode::OK,
        )),
        Err(e) => Err(reject::custom(RedisError(format!(
            "Redis storage error: {}",
            e
        )))),
    }
}
