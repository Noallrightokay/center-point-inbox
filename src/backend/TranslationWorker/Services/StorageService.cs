using Amazon.S3;
using Amazon.S3.Model;
using Amazon.S3.Util;
using CenterPointInbox.TranslationWorker.Configuration;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace CenterPointInbox.TranslationWorker.Services;

/// <summary>
/// S3-compatible storage service implementation using AWSSDK.S3.
/// Supports both AWS S3 and MinIO (via configurable endpoint and path-style addressing).
/// </summary>
public class StorageService : IStorageService
{
    private readonly IAmazonS3 _s3Client;
    private readonly S3Settings _s3Settings;
    private readonly ILogger<StorageService> _logger;

    public StorageService(
        IAmazonS3 s3Client,
        IOptions<TranslationSettings> settings,
        ILogger<StorageService> logger)
    {
        _s3Client = s3Client;
        _s3Settings = settings.Value.S3;
        _logger = logger;
    }

    public async Task<string> UploadAsync(byte[] content, string key, string contentType)
    {
        _logger.LogInformation(
            "Uploading object to S3. Bucket: {Bucket}, Key: {Key}, ContentType: {ContentType}, Size: {Size} bytes",
            _s3Settings.BucketName, key, contentType, content.Length);

        await EnsureBucketExistsAsync();

        using var memoryStream = new MemoryStream(content);

        var putRequest = new PutObjectRequest
        {
            BucketName = _s3Settings.BucketName,
            Key = key,
            InputStream = memoryStream,
            ContentType = contentType
        };

        await _s3Client.PutObjectAsync(putRequest);

        _logger.LogInformation(
            "Successfully uploaded object to S3. Bucket: {Bucket}, Key: {Key}",
            _s3Settings.BucketName, key);

        return key;
    }

    public async Task<byte[]> DownloadAsync(string key)
    {
        _logger.LogInformation(
            "Downloading object from S3. Bucket: {Bucket}, Key: {Key}",
            _s3Settings.BucketName, key);

        var getRequest = new GetObjectRequest
        {
            BucketName = _s3Settings.BucketName,
            Key = key
        };

        using var response = await _s3Client.GetObjectAsync(getRequest);
        using var memoryStream = new MemoryStream();
        await response.ResponseStream.CopyToAsync(memoryStream);

        var data = memoryStream.ToArray();

        _logger.LogInformation(
            "Successfully downloaded object from S3. Bucket: {Bucket}, Key: {Key}, Size: {Size} bytes",
            _s3Settings.BucketName, key, data.Length);

        return data;
    }

    public async Task DeleteAsync(string key)
    {
        _logger.LogInformation(
            "Deleting object from S3. Bucket: {Bucket}, Key: {Key}",
            _s3Settings.BucketName, key);

        var deleteRequest = new DeleteObjectRequest
        {
            BucketName = _s3Settings.BucketName,
            Key = key
        };

        await _s3Client.DeleteObjectAsync(deleteRequest);

        _logger.LogInformation(
            "Successfully deleted object from S3. Bucket: {Bucket}, Key: {Key}",
            _s3Settings.BucketName, key);
    }

    private async Task EnsureBucketExistsAsync()
    {
        try
        {
            var bucketExists = await AmazonS3Util.DoesS3BucketExistV2Async(_s3Client, _s3Settings.BucketName);
            if (!bucketExists)
            {
                _logger.LogInformation("Bucket '{Bucket}' does not exist, creating it", _s3Settings.BucketName);
                await _s3Client.PutBucketAsync(new PutBucketRequest
                {
                    BucketName = _s3Settings.BucketName
                });
                _logger.LogInformation("Bucket '{Bucket}' created successfully", _s3Settings.BucketName);
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex,
                "Could not ensure bucket '{Bucket}' exists. It may already exist or permissions may be restricted",
                _s3Settings.BucketName);
        }
    }
}
