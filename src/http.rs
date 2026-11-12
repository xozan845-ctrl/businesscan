//! Cliente HTTP con reintentos, backoff y límite de tasa por servicio.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;

use crate::config::Config;
use crate::normalize::truncate;

const MAX_ATTEMPTS: u32 = 4;

#[derive(Clone)]
pub struct HttpClient {
    client: Client,
}

impl HttpClient {
    pub fn new(cfg: &Config) -> Result<Self> {
        let client = Client::builder()
            .user_agent(cfg.user_agent.clone())
            .timeout(cfg.http_timeout)
            .connect_timeout(Duration::from_secs(20))
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .build()
            .context("No se pudo crear el cliente HTTP")?;
        Ok(Self { client })
    }

    pub fn raw(&self) -> &Client {
        &self.client
    }

    /// Ejecuta una petición reintentando 429, 5xx y errores de red.
    pub async fn execute<F>(&self, mut make: F) -> Result<Response>
    where
        F: FnMut() -> RequestBuilder,
    {
        let mut last_error: Option<anyhow::Error> = None;

        for attempt in 1..=MAX_ATTEMPTS {
            let request = make();
            match request.send().await {
                Ok(response) => {
                    let status = response.status();
                    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                        let retry_after = response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                            .map(Duration::from_secs);
                        let delay = retry_after.unwrap_or_else(|| backoff(attempt));
                        log::warn!(
                            "HTTP {} en intento {}/{}; reintentando en {:.1}s",
                            status,
                            attempt,
                            MAX_ATTEMPTS,
                            delay.as_secs_f64()
                        );
                        last_error = Some(anyhow::anyhow!("HTTP {status}"));
                        if attempt < MAX_ATTEMPTS {
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        return Ok(response);
                    }
                    return Ok(response);
                }
                Err(error) => {
                    log::warn!(
                        "Error de red en intento {}/{}: {}",
                        attempt,
                        MAX_ATTEMPTS,
                        error
                    );
                    last_error = Some(anyhow::Error::new(error));
                    if attempt < MAX_ATTEMPTS {
                        tokio::time::sleep(backoff(attempt)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Petición HTTP fallida")))
    }

    pub async fn json<T, F>(&self, make: F) -> Result<T>
    where
        T: DeserializeOwned,
        F: FnMut() -> RequestBuilder,
    {
        let response = self.execute(make).await?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("No se pudo leer la respuesta HTTP")?;
        if !status.is_success() {
            bail!(
                "HTTP {}: {}",
                status,
                truncate(body.trim(), 600).replace('\n', " ")
            );
        }
        serde_json::from_str(&body).with_context(|| {
            format!(
                "Respuesta JSON inválida: {}",
                truncate(body.trim(), 400).replace('\n', " ")
            )
        })
    }

    pub async fn text<F>(&self, make: F) -> Result<String>
    where
        F: FnMut() -> RequestBuilder,
    {
        let response = self.execute(make).await?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("No se pudo leer la respuesta HTTP")?;
        if !status.is_success() {
            bail!(
                "HTTP {}: {}",
                status,
                truncate(body.trim(), 600).replace('\n', " ")
            );
        }
        Ok(body)
    }
}

fn backoff(attempt: u32) -> Duration {
    let seconds = 2u64.pow(attempt.min(5));
    Duration::from_secs(seconds.min(30))
}

/// Limitador sencillo y compartido por host/servicio.
#[derive(Clone)]
pub struct RateLimiter {
    last: Arc<Mutex<Option<Instant>>>,
    interval: Duration,
}

impl RateLimiter {
    pub fn new(interval: Duration) -> Self {
        Self {
            last: Arc::new(Mutex::new(None)),
            interval,
        }
    }

    pub async fn wait(&self) {
        let mut guard = self.last.lock().await;
        if let Some(last) = *guard {
            let elapsed = last.elapsed();
            if elapsed < self.interval {
                tokio::time::sleep(self.interval - elapsed).await;
            }
        }
        *guard = Some(Instant::now());
    }
}
