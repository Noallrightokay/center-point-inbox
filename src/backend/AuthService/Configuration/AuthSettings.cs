namespace CenterPointInbox.AuthService.Configuration;

/// <summary>
/// Configuration POCO for OAuth provider settings and encryption.
/// Bound from the "Auth" section of appsettings.json.
/// </summary>
public class AuthSettings
{
    public const string SectionName = "Auth";

    /// <summary>
    /// Google OAuth 2.0 configuration.
    /// </summary>
    public GoogleOAuthSettings Google { get; set; } = new();

    /// <summary>
    /// Microsoft OAuth 2.0 / MSAL configuration.
    /// </summary>
    public MicrosoftOAuthSettings Microsoft { get; set; } = new();

    /// <summary>
    /// Base64-encoded 256-bit AES key used to encrypt OAuth tokens at rest.
    /// </summary>
    public string TokenEncryptionKey { get; set; } = string.Empty;
}

public class GoogleOAuthSettings
{
    public string ClientId { get; set; } = string.Empty;
    public string ClientSecret { get; set; } = string.Empty;
    public string TokenEndpoint { get; set; } = "https://oauth2.googleapis.com/token";
    public string UserInfoEndpoint { get; set; } = "https://www.googleapis.com/oauth2/v2/userinfo";
}

public class MicrosoftOAuthSettings
{
    public string ClientId { get; set; } = string.Empty;
    public string ClientSecret { get; set; } = string.Empty;
    public string TenantId { get; set; } = "common";
    public string TokenEndpoint { get; set; } = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
    public string UserInfoEndpoint { get; set; } = "https://graph.microsoft.com/v1.0/me";
}
