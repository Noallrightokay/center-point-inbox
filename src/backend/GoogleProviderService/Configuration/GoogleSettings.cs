namespace CenterPointInbox.GoogleProviderService.Configuration;

/// <summary>
/// Configuration POCO for Google API settings.
/// Bound from the "Google" section of appsettings.json.
/// </summary>
public class GoogleSettings
{
    public const string SectionName = "Google";

    /// <summary>
    /// Google OAuth 2.0 Client ID.
    /// </summary>
    public string ClientId { get; set; } = string.Empty;

    /// <summary>
    /// Google OAuth 2.0 Client Secret.
    /// </summary>
    public string ClientSecret { get; set; } = string.Empty;

    /// <summary>
    /// Google OAuth 2.0 Token Endpoint for refreshing tokens.
    /// </summary>
    public string TokenEndpoint { get; set; } = "https://oauth2.googleapis.com/token";

    /// <summary>
    /// Base64-encoded 256-bit AES key used to decrypt OAuth tokens read from the database.
    /// Must match the key used by the Auth Service to encrypt them.
    /// </summary>
    public string TokenEncryptionKey { get; set; } = string.Empty;

    /// <summary>
    /// Application name sent to Google API for request identification.
    /// </summary>
    public string ApplicationName { get; set; } = "CenterPointInbox";

    /// <summary>
    /// Default page size for file listing requests.
    /// </summary>
    public int DefaultPageSize { get; set; } = 100;

    /// <summary>
    /// Maximum page size allowed for file listing requests.
    /// </summary>
    public int MaxPageSize { get; set; } = 1000;
}
