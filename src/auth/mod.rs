use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};

use crate::config::Config;
use crate::error::{AppError, Result};
use crate::models::Claims;

/// JWT Authentication Service
#[derive(Clone)]
pub struct AuthService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    expiry_seconds: u64,
}

impl AuthService {
    pub fn new(config: &Config) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(config.jwt_secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(config.jwt_secret.as_bytes()),
            expiry_seconds: config.jwt_expiry_seconds,
        }
    }

    /// Generate a JWT token for a user joining a room
    pub fn generate_token(&self, user_id: &str, room_id: &str, display: &str) -> Result<String> {
        let now = Utc::now().timestamp();
        let exp = now + self.expiry_seconds as i64;

        let claims = Claims {
            sub: user_id.to_string(),
            room_id: room_id.to_string(),
            display: display.to_string(),
            iat: now,
            exp,
        };

        let token = encode(&Header::default(), &claims, &self.encoding_key)?;
        Ok(token)
    }

    /// Validate a JWT token and return the claims
    pub fn validate_token(&self, token: &str) -> Result<Claims> {
        let validation = Validation::default();
        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)
            .map_err(|e| AppError::Unauthorized(format!("Invalid token: {}", e)))?;

        Ok(token_data.claims)
    }

    /// Extract token from query string format: "token=xxx"
    pub fn extract_from_query(&self, query: &str) -> Result<Claims> {
        let token = query
            .split('&')
            .find_map(|pair| {
                let mut parts = pair.split('=');
                match (parts.next(), parts.next()) {
                    (Some("token"), Some(value)) => Some(value),
                    _ => None,
                }
            })
            .ok_or_else(|| AppError::Unauthorized("Token not found in query".to_string()))?;

        self.validate_token(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            server_host: "localhost".to_string(),
            server_port: 8080,
            redis_url: "redis://localhost".to_string(),
            jwt_secret: "test-secret-key".to_string(),
            jwt_expiry_seconds: 900,
            room_ttl_seconds: 7200,
            max_publishers_per_room: 50,
            stun_server: "stun:stun.l.google.com:19302".to_string(),
            turn_server: None,
            turn_username: None,
            turn_credential: None,
        }
    }

    #[test]
    fn test_generate_and_validate_token() {
        let config = test_config();
        let auth = AuthService::new(&config);

        let token = auth
            .generate_token("user-123", "room-456", "Alice")
            .expect("Should generate token");

        let claims = auth.validate_token(&token).expect("Should validate token");

        assert_eq!(claims.sub, "user-123");
        assert_eq!(claims.room_id, "room-456");
        assert_eq!(claims.display, "Alice");
    }

    #[test]
    fn test_extract_from_query() {
        let config = test_config();
        let auth = AuthService::new(&config);

        let token = auth
            .generate_token("user-123", "room-456", "Alice")
            .expect("Should generate token");

        let query = format!("room_id=room-456&token={}", token);
        let claims = auth
            .extract_from_query(&query)
            .expect("Should extract from query");

        assert_eq!(claims.sub, "user-123");
        assert_eq!(claims.room_id, "room-456");
    }

    #[test]
    fn test_invalid_token() {
        let config = test_config();
        let auth = AuthService::new(&config);

        let result = auth.validate_token("invalid-token");
        assert!(result.is_err());
    }
}
