namespace CenterPointInbox.TranslationWorker.Configuration;

/// <summary>
/// Configuration POCO for translation worker settings.
/// Bound from the "Translation" section of appsettings.json.
/// </summary>
public class TranslationSettings
{
    public const string SectionName = "Translation";

    /// <summary>
    /// S3-compatible storage configuration (supports MinIO).
    /// </summary>
    public S3Settings S3 { get; set; } = new();

    /// <summary>
    /// URLs for provider services used to download source files.
    /// </summary>
    public ProviderServiceUrls Providers { get; set; } = new();

    /// <summary>
    /// Maximum file size in bytes allowed for translation. Defaults to 100 MB.
    /// </summary>
    public long MaxFileSizeBytes { get; set; } = 104_857_600;

    /// <summary>
    /// Timeout in seconds for downloading source files from provider services. Defaults to 120 seconds.
    /// </summary>
    public int DownloadTimeoutSeconds { get; set; } = 120;
}

/// <summary>
/// S3-compatible storage configuration settings.
/// </summary>
public class S3Settings
{
    /// <summary>
    /// S3-compatible endpoint URL (e.g., http://minio:9000 for MinIO).
    /// </summary>
    public string Endpoint { get; set; } = string.Empty;

    /// <summary>
    /// Bucket name for storing translated files.
    /// </summary>
    public string BucketName { get; set; } = "translations";

    /// <summary>
    /// Access key for S3 authentication.
    /// </summary>
    public string AccessKey { get; set; } = string.Empty;

    /// <summary>
    /// Secret key for S3 authentication.
    /// </summary>
    public string SecretKey { get; set; } = string.Empty;

    /// <summary>
    /// AWS region for the S3 bucket. Defaults to us-east-1.
    /// </summary>
    public string Region { get; set; } = "us-east-1";

    /// <summary>
    /// Whether to force path-style addressing (required for MinIO). Defaults to true.
    /// </summary>
    public bool ForcePathStyle { get; set; } = true;
}

/// <summary>
/// URLs for external provider services that host source files.
/// </summary>
public class ProviderServiceUrls
{
    /// <summary>
    /// Base URL for the Google Provider Service (Google Docs downloads).
    /// </summary>
    public string GoogleProviderService { get; set; } = "http://google-provider-service";

    /// <summary>
    /// Base URL for the Microsoft Provider Service (Word document downloads).
    /// </summary>
    public string MicrosoftProviderService { get; set; } = "http://microsoft-provider-service";
}
