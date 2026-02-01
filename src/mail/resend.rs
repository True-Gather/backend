use crate::error::{AppError, Result};
use reqwest::Client;
use serde::Serialize;

#[derive(Clone)]
pub struct ResendMailer {
    client: Client,
    api_key: String,
    from: String,
}

impl ResendMailer {
    pub fn new_from_env() -> Result<Self> {
        let api_key = std::env::var("RESEND_API_KEY")
            .map_err(|_| AppError::BadRequest("RESEND_API_KEY missing in env".to_string()))?;

        // Pour MVP: tu peux utiliser onboarding@resend.dev (ou ton domaine vérifié)
        let from = std::env::var("MAIL_FROM")
            .unwrap_or_else(|_| "TrueGather <onboarding@resend.dev>".to_string());

        Ok(Self {
            client: Client::new(),
            api_key,
            from,
        })
    }

    pub async fn send(&self, to: Vec<String>, subject: String, text: String) -> Result<()> {
        #[derive(Serialize)]
        struct Payload {
            from: String,
            to: Vec<String>,
            subject: String,
            text: String,
        }

        let payload = Payload {
            from: self.from.clone(),
            to,
            subject,
            text,
        };

        let res = self
            .client
            .post("https://api.resend.com/emails")
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::BadRequest(format!("Mail send failed: {}", e)))?;

        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(AppError::BadRequest(format!(
                "Resend API error: {}",
                body
            )));
        }

        Ok(())
    }
}
