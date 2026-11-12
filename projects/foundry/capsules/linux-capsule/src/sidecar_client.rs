use crate::{Error, Input, Result};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct SidecarClient {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl SidecarClient {
    pub fn new(port: u16, token: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        Self {
            http,
            base: format!("http://127.0.0.1:{port}"),
            token,
        }
    }

    pub async fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let error = match self.health().await {
                Ok(()) => return Ok(()),
                Err(error) => error,
            };
            if Instant::now() >= deadline {
                return Err(Error::NotReady(format!(
                    "capsule did not answer /health within {timeout:?}: {error}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn health(&self) -> Result<()> {
        let response = self
            .http
            .get(format!("{}/health", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|error| Error::Backend(error.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Backend(format!(
                "health returned {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }
        Ok(())
    }

    pub async fn screenshot(&self) -> Result<Vec<u8>> {
        let response = self
            .http
            .get(format!("{}/api/screenshot", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|error| Error::Backend(error.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Backend(format!(
                "screenshot returned {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|error| Error::Backend(error.to_string()))
    }

    pub async fn input(&self, input: &Input) -> Result<()> {
        let response = self
            .http
            .post(format!("{}/api/input", self.base))
            .bearer_auth(&self.token)
            .json(input)
            .send()
            .await
            .map_err(|error| Error::Backend(error.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Backend(format!(
                "input returned {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }
        Ok(())
    }
}
