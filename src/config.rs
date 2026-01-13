use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub server_host: String,
    pub server_port: u16,
    pub redis_url: String,
    pub jwt_secret: String,
    pub jwt_expiry_seconds: u64,
    pub room_ttl_seconds: u64,
    pub max_publishers_per_room: u32,
    pub stun_server: String,
    pub turn_server: Option<String>,
    pub turn_username: Option<String>,
    pub turn_credential: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();

        Ok(Config {
            server_host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            server_port: env::var("SERVER_PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .map_err(|_| ConfigError::InvalidPort)?,
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            jwt_secret: env::var("JWT_SECRET").map_err(|_| ConfigError::MissingJwtSecret)?,
            jwt_expiry_seconds: env::var("JWT_EXPIRY_SECONDS")
                .unwrap_or_else(|_| "900".to_string())
                .parse()
                .unwrap_or(900),
            room_ttl_seconds: env::var("ROOM_TTL_SECONDS")
                .unwrap_or_else(|_| "7200".to_string())
                .parse()
                .unwrap_or(7200),
            max_publishers_per_room: env::var("MAX_PUBLISHERS_PER_ROOM")
                .unwrap_or_else(|_| "50".to_string())
                .parse()
                .unwrap_or(50),
            stun_server: env::var("STUN_SERVER")
                .unwrap_or_else(|_| "stun:stun.l.google.com:19302".to_string()),
            turn_server: env::var("TURN_SERVER").ok(),
            turn_username: env::var("TURN_USERNAME").ok(),
            turn_credential: env::var("TURN_CREDENTIAL").ok(),
        })
    }

    pub fn server_addr(&self) -> String {
        format!("{}:{}", self.server_host, self.server_port)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Invalid server port")]
    InvalidPort,
    #[error("JWT_SECRET environment variable is required")]
    MissingJwtSecret,
}
