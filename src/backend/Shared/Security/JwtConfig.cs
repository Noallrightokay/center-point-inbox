namespace CenterPointInbox.Shared.Security;

/// <summary>
/// Configuration POCO for JWT authentication settings.
/// Bound from the "Jwt" section of appsettings.json.
/// </summary>
public class JwtConfig
{
    public const string SectionName = "Jwt";

    /// <summary>
    /// The issuer (iss) claim for generated tokens.
    /// </summary>
    public string Issuer { get; set; } = string.Empty;

    /// <summary>
    /// The audience (aud) claim for generated tokens.
    /// </summary>
    public string Audience { get; set; } = string.Empty;

    /// <summary>
    /// The symmetric signing key used to sign and validate tokens.
    /// Must be at least 256 bits (32 bytes) for HMAC-SHA256.
    /// </summary>
    public string SigningKey { get; set; } = string.Empty;

    /// <summary>
    /// Access token expiration in minutes. Defaults to 15 minutes.
    /// </summary>
    public int ExpirationMinutes { get; set; } = 15;

    /// <summary>
    /// Refresh token expiration in days. Defaults to 7 days.
    /// </summary>
    public int RefreshExpirationDays { get; set; } = 7;
}
