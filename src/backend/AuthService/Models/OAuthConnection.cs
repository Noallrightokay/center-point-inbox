namespace CenterPointInbox.AuthService.Models;

/// <summary>
/// Represents an OAuth provider connection for a user.
/// Stores encrypted access/refresh tokens for each connected provider.
/// </summary>
public class OAuthConnection
{
    public Guid Id { get; set; }

    public Guid UserId { get; set; }

    public Provider Provider { get; set; }

    /// <summary>
    /// The unique identifier for the user at the OAuth provider (e.g., Google sub, Microsoft oid).
    /// </summary>
    public string ProviderUserId { get; set; } = string.Empty;

    /// <summary>
    /// The email address associated with this provider account.
    /// </summary>
    public string ProviderEmail { get; set; } = string.Empty;

    /// <summary>
    /// AES-256-GCM encrypted OAuth access token.
    /// </summary>
    public string EncryptedAccessToken { get; set; } = string.Empty;

    /// <summary>
    /// AES-256-GCM encrypted OAuth refresh token.
    /// </summary>
    public string? EncryptedRefreshToken { get; set; }

    /// <summary>
    /// When the OAuth access token expires.
    /// </summary>
    public DateTimeOffset? AccessTokenExpiresAt { get; set; }

    /// <summary>
    /// OAuth scopes granted by the user for this provider.
    /// </summary>
    public string Scopes { get; set; } = string.Empty;

    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;

    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;

    // Navigation properties
    public User User { get; set; } = null!;
}

/// <summary>
/// Supported OAuth providers.
/// </summary>
public enum Provider
{
    Google = 0,
    Microsoft = 1
}
