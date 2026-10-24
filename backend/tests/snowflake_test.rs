use std::collections::HashSet;
use std::sync::Arc;
use url_shortener::handlers::{generate_short_url_id, SnowflakeGenerator};

/// 8 threads x 50k IDs from one generator: every ID must be unique.
#[test]
fn snowflake_ids_are_unique_under_concurrency() {
    let gen = Arc::new(SnowflakeGenerator::new(1));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let g = gen.clone();
            std::thread::spawn(move || (0..50_000).map(|_| g.generate()).collect::<Vec<_>>())
        })
        .collect();
    let mut seen = HashSet::new();
    for h in handles {
        for id in h.join().unwrap() {
            assert!(seen.insert(id), "duplicate snowflake id {id}");
        }
    }
    assert_eq!(seen.len(), 400_000);
}

/// Different node ids (one per backend) never collide, even in the same millisecond.
#[test]
fn snowflake_ids_are_unique_across_nodes() {
    let nodes: Vec<_> = (1..=3).map(SnowflakeGenerator::new).collect();
    let mut seen = HashSet::new();
    for _ in 0..20_000 {
        for n in &nodes {
            assert!(seen.insert(n.generate()));
        }
    }
}

/// Short codes are the full base62 Snowflake ID, so they are unique too.
#[test]
fn short_codes_are_unique() {
    let mut seen = HashSet::new();
    for _ in 0..100_000 {
        assert!(seen.insert(generate_short_url_id("https://example.com")));
    }
}
