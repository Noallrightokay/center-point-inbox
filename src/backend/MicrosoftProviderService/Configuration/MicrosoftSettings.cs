namespace CenterPointInbox.MicrosoftProviderService.Configuration;

/// <summary>
/// Configuration POCO for Microsoft Graph API settings.
/// Bound from the "Microsoft" section of appsettings.json.
/// </summary>
public class MicrosoftSettings
{
    public const string SectionName = "Microsoft";

    /// <summary>
    /// Azure AD Application (client) ID.
    /// </summary>
    public string ClientId { get; set; } = string.Empty;

    /// <summary>
    /// Azure AD Application client secret.
    /// </summary>
    public string ClientSecret { get; set; } = string.Empty;

    /// <summary>
    /// Azure AD Tenant ID. Use "common" for multi-tenant applications.
    /// </summary>
    public string TenantId { get; set; } = "common";

    /// <summary>
    /// Microsoft OAuth 2.0 Token Endpoint for refreshing tokens.
    /// </summary>
    public string TokenEndpoint { get; set; } = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

    /// <summary>
    /// Base64-encoded 256-bit AES key used to decrypt OAuth tokens read from the database.
    /// Must match the key used by the Auth Service to encrypt them.
    /// </summary>
    public string TokenEncryptionKey { get; set; } = string.Empty;

    /// <summary>
    /// Default page size for file listing requests.
    /// </summary>
    public int DefaultPageSize { get; set; } = 100;

    /// <summary>
    /// Maximum page size allowed for file listing requests.
    /// </summary>
    public int MaxPageSize { get; set; } = 1000;

    /// <summary>
    /// Microsoft Graph API base URL.
    /// </summary>
    public string GraphBaseUrl { get; set; } = "https://graph.microsoft.com/v1.0";
}
