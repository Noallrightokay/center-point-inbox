namespace CenterPointInbox.TranslationWorker.Services;

/// <summary>
/// Abstraction for S3-compatible object storage operations.
/// Supports MinIO and AWS S3 backends via configurable endpoint.
/// </summary>
public interface IStorageService
{
    /// <summary>
    /// Uploads content to S3-compatible storage.
    /// </summary>
    /// <param name="content">The raw bytes to upload.</param>
    /// <param name="key">The object key (path) within the bucket.</param>
    /// <param name="contentType">The MIME type of the content.</param>
    /// <returns>The S3 key of the uploaded object.</returns>
    Task<string> UploadAsync(byte[] content, string key, string contentType);

    /// <summary>
    /// Downloads content from S3-compatible storage.
    /// </summary>
    /// <param name="key">The object key (path) within the bucket.</param>
    /// <returns>The raw bytes of the stored object.</returns>
    Task<byte[]> DownloadAsync(string key);

    /// <summary>
    /// Deletes an object from S3-compatible storage.
    /// </summary>
    /// <param name="key">The object key (path) within the bucket to delete.</param>
    Task DeleteAsync(string key);
}
