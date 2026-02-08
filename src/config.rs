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

    pub mail_from: Option<String>,
    pub resend_api_key: Option<String>,

    pub frontend_host: Option<String>,
    pub frontend_port: Option<u16>,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        // Load .env (best-effort)
        dotenvy::from_filename(".env").ok();
        dotenvy::dotenv().ok();

        let server_host = env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let server_port: u16 = env::var("SERVER_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);

        let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

        let jwt_secret = env::var("JWT_SECRET").map_err(|_| ConfigError::MissingJwtSecret)?;
        let jwt_expiry_seconds: u64 = env::var("JWT_EXPIRY_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(86400);

        let room_ttl_seconds: u64 = env::var("ROOM_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(7200);

        let max_publishers_per_room: u32 = env::var("MAX_PUBLISHERS_PER_ROOM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4);

        let stun_server = env::var("STUN_SERVER").unwrap_or_else(|_| "stun:stun.l.google.com:19302".to_string());

        let turn_server = env::var("TURN_SERVER").ok();
        let turn_username = env::var("TURN_USERNAME").ok();
        let turn_credential = env::var("TURN_CREDENTIAL").ok();

        let mail_from = env::var("MAIL_FROM").ok();
        let resend_api_key = env::var("RESEND_API_KEY").ok();

        let frontend_host = env::var("FRONTEND_HOST").ok();
        let frontend_port = env::var("FRONTEND_PORT").ok().and_then(|v| v.parse::<u16>().ok());

        Ok(Self {
            server_host,
            server_port,
            redis_url,
            jwt_secret,
            jwt_expiry_seconds,
            room_ttl_seconds,
            max_publishers_per_room,
            stun_server,
            turn_server,
            turn_username,
            turn_credential,
            mail_from,
            resend_api_key,
            frontend_host,
            frontend_port,
        })
    }

    /// Used by main.rs: "host:port"
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
