pub mod room_repository;

pub use room_repository::*;

use deadpool_redis::{Config as RedisConfig, Pool, Runtime};

use crate::config::Config;
use crate::error::{AppError, Result};

/// Create a Redis connection pool
pub fn create_pool(config: &Config) -> Result<Pool> {
    let redis_config = RedisConfig::from_url(&config.redis_url);
    let pool = redis_config
        .create_pool(Some(Runtime::Tokio1))
        .map_err(|e| AppError::RedisError(format!("Failed to create Redis pool: {}", e)))?;

    Ok(pool)
}
