pub mod resend;

use crate::error::Result;

#[derive(Clone)]
pub struct Mailer {
    inner: resend::ResendMailer,
}

impl Mailer {
    pub fn new_from_env() -> Result<Self> {
        Ok(Self {
            inner: resend::ResendMailer::new_from_env()?,
        })
    }

    pub async fn send_invite(&self, to: Vec<String>, subject: String, text: String) -> Result<()> {
        self.inner.send(to, subject, text).await
    }
}
