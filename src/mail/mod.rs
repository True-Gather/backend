pub mod resend;

use crate::error::Result;

/// Mailer abstraction (currently backed by Resend)
#[derive(Clone)]
pub struct Mailer {
    inner: resend::ResendMailer,
}

impl Mailer {
    /// Create mailer from env (RESEND_API_KEY, MAIL_FROM, etc.)
    pub fn new_from_env() -> Result<Self> {
        Ok(Self {
            inner: resend::ResendMailer::new_from_env()?,
        })
    }

    /// Send invitation email(s)
    pub async fn send_invite(&self, to: Vec<String>, subject: String, text: String) -> Result<()> {
        self.inner.send(to, subject, text).await
    }
}
