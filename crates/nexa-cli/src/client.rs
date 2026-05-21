use anyhow::Result;
use reqwest::Client;
use serde::de::DeserializeOwned;

pub struct NexaClient {
    base_url: String,
    http: Client,
}

impl NexaClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: Client::new(),
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{path}", self.base_url);
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("request failed ({status}): {body}");
        }
        Ok(resp.json().await?)
    }

    pub async fn post_json<T: DeserializeOwned>(&self, path: &str, body: &str) -> Result<T> {
        let url = format!("{}{path}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("request failed ({status}): {body}");
        }
        Ok(resp.json().await?)
    }

    pub async fn post_yaml<T: DeserializeOwned>(&self, path: &str, body: &str) -> Result<T> {
        let url = format!("{}{path}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("content-type", "application/yaml")
            .body(body.to_string())
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("request failed ({status}): {body}");
        }
        Ok(resp.json().await?)
    }

    pub async fn post_empty(&self, path: &str) -> Result<()> {
        let url = format!("{}{path}", self.base_url);
        let resp = self.http.post(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("request failed ({status}): {body}");
        }
        Ok(())
    }

    pub async fn delete(&self, path: &str) -> Result<()> {
        let url = format!("{}{path}", self.base_url);
        let resp = self.http.delete(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("request failed ({status}): {body}");
        }
        Ok(())
    }

    pub async fn get_stream(&self, path: &str) -> Result<reqwest::Response> {
        let url = format!("{}{path}", self.base_url);
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("request failed ({status}): {body}");
        }
        Ok(resp)
    }
}
