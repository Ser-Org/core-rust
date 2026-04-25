//! Live integration smoke test: PUTs a tiny file through the ObjectStore
//! wrapper using whatever S3_* values are in `.env`. Run with:
//!
//!     cargo test --release --test s3_upload_smoke -- --nocapture --ignored
//!
//! Marked `#[ignore]` so it doesn't run in normal `cargo test`.

#[tokio::test]
#[ignore]
async fn upload_tiny_file() {
    dotenvy::dotenv().ok();
    let endpoint = std::env::var("S3_ENDPOINT").unwrap();
    let access = std::env::var("S3_ACCESS_KEY").unwrap();
    let secret = std::env::var("S3_SECRET_KEY").unwrap();
    let region = std::env::var("S3_REGION").unwrap_or_else(|_| "local".into());
    let bucket = std::env::var("S3_BUCKET").unwrap();

    println!("endpoint = {}", endpoint);
    println!("bucket   = {}", bucket);
    println!("region   = {}", region);
    println!("access   = {}...", &access[..8.min(access.len())]);

    let mut store = scout_core::objectstore::ObjectStore::new(&endpoint, &access, &secret, &region)
        .await
        .expect("construct ObjectStore");
    let supabase_url = std::env::var("SUPABASE_URL").unwrap();
    let supabase_service_key = std::env::var("SUPABASE_SERVICE_KEY").unwrap();
    store.set_supabase_credentials(&supabase_url, &supabase_service_key);

    let data = vec![0u8; 128];
    let path = format!("smoke-test/{}.bin", uuid::Uuid::new_v4());
    match store
        .upload(&bucket, &path, data, "application/octet-stream")
        .await
    {
        Ok(url) => {
            println!("upload OK: {}", url);
            let _ = store.delete(&bucket, &path).await;
        }
        Err(e) => {
            println!("upload failed: {:#}", e);
            panic!("{:#}", e);
        }
    }
}
