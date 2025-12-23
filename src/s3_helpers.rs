use bytes::Bytes;
use s3::Bucket;
use s3::error::S3Error;

pub async fn check_file_exists(bucket: &Bucket, key: &str) -> Result<bool, S3Error> {
    bucket.object_exists(key).await
}

pub async fn upload_file(bucket: &Bucket, key: &str, data: Bytes) -> Result<(), S3Error> {
    bucket.put_object(key, &data).await?;
    Ok(())
}

pub async fn generate_presigned_url(
    bucket: &Bucket,
    key: &str,
    expiry_seconds: u32,
) -> Result<String, S3Error> {
    bucket.presign_get(key, expiry_seconds, None).await
}
