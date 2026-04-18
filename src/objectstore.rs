//! Object store backed by Supabase Storage's REST API.
//!
//! We originally used the AWS S3-compatible SDK against Supabase's
//! `/storage/v1/s3` endpoint, but the Rust aws-sdk-s3 crate rejects the
//! non-XML responses that Supabase returns for several operations (`error
//! parsing XML: no root element`). The Go client is lenient enough to
//! tolerate this; Rust's SDK is not. The Storage REST API
//! (`/storage/v1/object/…`) is the authoritative, officially-supported
//! interface and works with every Supabase deployment.

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct ObjectStore {
    http: Client,
    /// Base URL of the Supabase project — e.g. `http://127.0.0.1:54321`.
    /// We accept either the bare project URL or the legacy `/storage/v1/s3`
    /// endpoint form; in the latter case we strip the S3 suffix so REST paths
    /// line up.
    pub base_url: String,
    pub service_key: String,
}

impl ObjectStore {
    pub async fn new(
        endpoint_url: &str,
        _access_key: &str,
        _secret_key: &str,
        _region: &str,
    ) -> Result<Self> {
        // Strip optional `/storage/v1/s3` suffix so the base URL is just the
        // Supabase project root.
        let base = endpoint_url
            .trim_end_matches('/')
            .trim_end_matches("/storage/v1/s3")
            .trim_end_matches("/storage/v1")
            .trim_end_matches('/')
            .to_string();
        let http = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            base_url: base,
            service_key: String::new(),
        })
    }

    pub fn set_supabase_credentials(&mut self, url: &str, service_key: &str) {
        // Prefer the Supabase URL passed via SUPABASE_URL over whatever came
        // through the S3 endpoint form.
        let url = url.trim_end_matches('/');
        if !url.is_empty() {
            self.base_url = url.to_string();
        }
        self.service_key = service_key.to_string();
    }

    fn object_url(&self, bucket: &str, path: &str) -> String {
        format!("{}/storage/v1/object/{}/{}", self.base_url, bucket, path)
    }

    fn bucket_url(&self, bucket: &str) -> String {
        format!("{}/storage/v1/bucket/{}", self.base_url, bucket)
    }

    fn auth_headers<'a>(&'a self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        // Supabase Storage auth accepts both legacy JWT (`eyJ…`) and the
        // newer `sb_secret_…` keys. The newer keys must also be sent as
        // `apikey`; the JWT form doesn't need it but accepts it harmlessly.
        builder
            .bearer_auth(&self.service_key)
            .header("apikey", &self.service_key)
    }

    /// HEAD the bucket to verify existence; create it if missing.
    /// Creating buckets on Supabase requires the service role key; local
    /// CLI users may need to pre-create buckets via the dashboard.
    pub async fn ensure_bucket(&self, bucket: &str) -> Result<()> {
        let resp = self
            .auth_headers(self.http.get(self.bucket_url(bucket)))
            .send()
            .await
            .with_context(|| format!("ensure_bucket: HEAD {}", bucket))?;
        if resp.status().is_success() {
            return Ok(());
        }
        // Try create.
        #[derive(Serialize)]
        struct Body<'a> {
            id: &'a str,
            name: &'a str,
            public: bool,
        }
        let create_url = format!("{}/storage/v1/bucket", self.base_url);
        let resp = self
            .auth_headers(self.http.post(&create_url))
            .json(&Body { id: bucket, name: bucket, public: true })
            .send()
            .await
            .with_context(|| format!("ensure_bucket: POST /bucket {}", bucket))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            // 409 Conflict = already exists; treat as success.
            if status == StatusCode::CONFLICT || body.contains("already exists") {
                return Ok(());
            }
            return Err(anyhow!(
                "ensure_bucket: create {} -> {}: {}",
                bucket,
                status,
                body
            ));
        }
        Ok(())
    }

    /// Upload bytes and return the public URL for the object.
    pub async fn upload(
        &self,
        bucket: &str,
        path: &str,
        data: Vec<u8>,
        content_type: &str,
    ) -> Result<String> {
        let url = self.object_url(bucket, path);
        let resp = self
            .auth_headers(self.http.post(&url))
            .header("Content-Type", content_type)
            .header("x-upsert", "true")
            .body(data)
            .send()
            .await
            .with_context(|| format!("objectstore: upload {}/{}", bucket, path))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "objectstore: upload {}/{} -> {}: {}",
                bucket,
                path,
                status,
                body
            ));
        }
        // Return the public URL suitable for storage_url columns. Callers
        // that need authenticated access use get_signed_url() separately.
        Ok(format!(
            "{}/storage/v1/object/public/{}/{}",
            self.base_url, bucket, path
        ))
    }

    pub async fn download(&self, bucket: &str, path: &str) -> Result<Vec<u8>> {
        let url = format!(
            "{}/storage/v1/object/authenticated/{}/{}",
            self.base_url, bucket, path
        );
        let resp = self
            .auth_headers(self.http.get(&url))
            .send()
            .await
            .with_context(|| format!("objectstore: download {}/{}", bucket, path))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "objectstore: download {}/{} -> {}: {}",
                bucket,
                path,
                status,
                body
            ));
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn head_object_content_type(&self, bucket: &str, path: &str) -> Result<String> {
        let url = format!(
            "{}/storage/v1/object/authenticated/{}/{}",
            self.base_url, bucket, path
        );
        let resp = self
            .auth_headers(self.http.request(reqwest::Method::HEAD, &url))
            .send()
            .await
            .with_context(|| format!("objectstore: head {}/{}", bucket, path))?;
        if !resp.status().is_success() {
            return Err(anyhow!("objectstore: head {}/{}: {}", bucket, path, resp.status()));
        }
        Ok(resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string())
    }

    pub async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
        limit: i32,
    ) -> Result<Vec<String>> {
        #[derive(Serialize)]
        struct Body<'a> {
            prefix: &'a str,
            limit: i32,
        }
        #[derive(Deserialize)]
        struct Entry {
            name: String,
        }
        let url = format!("{}/storage/v1/object/list/{}", self.base_url, bucket);
        let resp = self
            .auth_headers(self.http.post(&url))
            .json(&Body { prefix, limit })
            .send()
            .await
            .with_context(|| format!("objectstore: list {}/{}", bucket, prefix))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("objectstore: list -> {}: {}", status, body));
        }
        let items: Vec<Entry> = resp.json().await?;
        Ok(items.into_iter().map(|e| e.name).collect())
    }

    pub async fn delete(&self, bucket: &str, path: &str) -> Result<()> {
        let url = self.object_url(bucket, path);
        let resp = self
            .auth_headers(self.http.delete(&url))
            .send()
            .await
            .with_context(|| format!("objectstore: delete {}/{}", bucket, path))?;
        let status = resp.status();
        if !status.is_success() && status != StatusCode::NOT_FOUND {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "objectstore: delete {}/{} -> {}: {}",
                bucket,
                path,
                status,
                body
            ));
        }
        Ok(())
    }

    /// Returns a short-lived signed URL that the frontend (or external
    /// services like Runway) can fetch without any auth header.
    pub async fn get_signed_url(
        &self,
        bucket: &str,
        path: &str,
        expires_in: Duration,
    ) -> Result<String> {
        create_supabase_signed_url(
            &self.base_url,
            &self.service_key,
            bucket,
            path,
            expires_in.as_secs() as i64,
        )
        .await
    }

    /// External providers that cannot reach localhost (e.g. Runway, Flux)
    /// need the asset inline. When the Supabase base URL is local we embed
    /// the asset as a base64 data URI; otherwise we fall back to a signed
    /// URL on the public hosted domain.
    pub async fn get_external_signed_url(
        &self,
        bucket: &str,
        path: &str,
        expires_in: Duration,
    ) -> Result<String> {
        if is_local_host(&self.base_url) {
            return self.to_data_uri(bucket, path).await;
        }
        self.get_signed_url(bucket, path, expires_in).await
    }

    async fn to_data_uri(&self, bucket: &str, path: &str) -> Result<String> {
        let data = self.download(bucket, path).await?;
        let mime = match self.head_object_content_type(bucket, path).await {
            Ok(m) if !m.is_empty() => m,
            _ => guess_mime_type_from_path(path),
        };
        let encoded = B64.encode(&data);
        Ok(format!("data:{};base64,{}", mime, encoded))
    }
}

pub async fn create_supabase_signed_url(
    base_url: &str,
    service_key: &str,
    bucket: &str,
    path: &str,
    expires_in: i64,
) -> Result<String> {
    let endpoint = format!(
        "{}/storage/v1/object/sign/{}/{}",
        base_url.trim_end_matches('/'),
        bucket,
        path
    );
    let body = format!(r#"{{"expiresIn":{}}}"#, expires_in);
    let client = Client::new();
    let resp = client
        .post(&endpoint)
        .bearer_auth(service_key)
        .header("apikey", service_key)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .with_context(|| format!("sign {}/{}", bucket, path))?;
    let status = resp.status();
    let raw = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "objectstore: supabase sign {}/{} -> {}: {}",
            bucket,
            path,
            status,
            raw
        ));
    }
    #[derive(Deserialize)]
    struct R {
        #[serde(rename = "signedURL")]
        signed_url: Option<String>,
    }
    let r: R = serde_json::from_str(&raw)
        .with_context(|| format!("sign {}/{}: decode response {}", bucket, path, raw))?;
    let signed = r
        .signed_url
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("objectstore: supabase sign returned empty URL"))?;
    // Supabase returns a relative URL — historically `/storage/v1/object/sign/...`
    // but current Supabase builds return `/object/sign/...` without the
    // `/storage/v1` prefix. Normalize to always point at the REST endpoint.
    if signed.starts_with("http://") || signed.starts_with("https://") {
        return Ok(signed);
    }
    let rel = signed.trim_start_matches('/');
    let rel = rel.trim_start_matches("storage/v1/");
    Ok(format!(
        "{}/storage/v1/{}",
        base_url.trim_end_matches('/'),
        rel
    ))
}

fn is_local_host(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains("127.0.0.1") || lower.contains("localhost") || lower.contains("host.docker.internal")
}

pub fn guess_mime_type_from_path(path: &str) -> String {
    if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg".into()
    } else if path.ends_with(".webp") {
        "image/webp".into()
    } else if path.ends_with(".mp4") {
        "video/mp4".into()
    } else if path.ends_with(".mp3") {
        "audio/mpeg".into()
    } else {
        "image/png".into()
    }
}
