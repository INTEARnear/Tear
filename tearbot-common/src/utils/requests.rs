use cached::proc_macro::{cached, io_cached};
use lazy_static::lazy_static;
use reqwest::{IntoUrl, Url};
use serde::de::DeserializeOwned;

lazy_static! {
    static ref CLIENT: reqwest::Client = reqwest::Client::builder()
        .user_agent("Intear Xeon")
        .build()
        .expect("Failed to create reqwest client");
}

pub fn get_reqwest_client() -> &'static reqwest::Client {
    &CLIENT
}

async fn _get_internal(uri: &str) -> Result<serde_json::Value, anyhow::Error> {
    Ok(get_reqwest_client().get(uri).send().await?.json().await?)
}

#[cached(time = 30, result = true, size = 50)]
async fn _get_cached_30s(uri: String) -> Result<serde_json::Value, anyhow::Error> {
    _get_internal(&uri).await
}

#[cached(time = 300, result = true, size = 50)]
async fn _get_cached_5m(uri: String) -> Result<serde_json::Value, anyhow::Error> {
    _get_internal(&uri).await
}

#[cached(time = 3600, result = true, size = 50)]
async fn _get_cached_1h(uri: String) -> Result<serde_json::Value, anyhow::Error> {
    _get_internal(&uri).await
}

#[cached(time = 86400, result = true, size = 50)]
async fn _get_cached_1d(uri: String) -> Result<serde_json::Value, anyhow::Error> {
    _get_internal(&uri).await
}

pub async fn get_cached_30s<O: DeserializeOwned>(uri: &str) -> Result<O, anyhow::Error> {
    let res = _get_cached_30s(uri.to_string()).await?;
    Ok(serde_json::from_value(res)?)
}

pub async fn get_cached_5m<O: DeserializeOwned>(uri: &str) -> Result<O, anyhow::Error> {
    let res = _get_cached_5m(uri.to_string()).await?;
    Ok(serde_json::from_value(res)?)
}

pub async fn get_cached_1h<O: DeserializeOwned>(uri: &str) -> Result<O, anyhow::Error> {
    let res = _get_cached_1h(uri.to_string()).await?;
    Ok(serde_json::from_value(res)?)
}

pub async fn get_cached_1d<O: DeserializeOwned>(uri: &str) -> Result<O, anyhow::Error> {
    let res = _get_cached_1d(uri.to_string()).await?;
    Ok(serde_json::from_value(res)?)
}

pub async fn get_not_cached<O: DeserializeOwned>(uri: &str) -> Result<O, anyhow::Error> {
    let res = _get_internal(uri).await?;
    Ok(serde_json::from_value(res)?)
}

pub async fn fetch_file(url: impl IntoUrl) -> Result<Vec<u8>, anyhow::Error> {
    let mut response = get_reqwest_client().get(url).send().await?;

    const LIMIT_FOR_FILES: usize = 10 * 1024 * 1024; // 10 MB

    let mut bytes = Vec::new();
    loop {
        let chunk = response.chunk().await?;
        match chunk {
            Some(chunk) => {
                if bytes.len() + chunk.len() > LIMIT_FOR_FILES {
                    return Err(anyhow::anyhow!("File is too big"));
                }
                bytes.extend_from_slice(&chunk);
            }
            None => break,
        }
    }
    Ok(bytes)
}

#[io_cached(time = 300, disk = true, map_error = r#"|e| anyhow::Error::from(e)"#)]
pub async fn fetch_file_cached_5m(url: Url) -> Result<Vec<u8>, anyhow::Error> {
    fetch_file(url).await
}

#[io_cached(time = 3600, disk = true, map_error = r#"|e| anyhow::Error::from(e)"#)]
pub async fn fetch_file_cached_1h(url: Url) -> Result<Vec<u8>, anyhow::Error> {
    fetch_file(url).await
}

#[io_cached(time = 86400, disk = true, map_error = r#"|e| anyhow::Error::from(e)"#)]
pub async fn fetch_file_cached_1d(url: Url) -> Result<Vec<u8>, anyhow::Error> {
    fetch_file(url).await
}
