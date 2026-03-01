using System.Text.Json.Serialization;

namespace CenterPointInbox.AuthService.Models;

/// <summary>
/// Request DTO for initiating OAuth login flow.
/// The client sends the auth code received from the OAuth provider callback.
/// </summary>
public class LoginRequest
{
    /// <summary>
    /// The OAuth provider used for authentication.
    /// </summary>
    [JsonPropertyName("provider")]
    public Provider Provider { get; init; }

    /// <summary>
    /// The authorization code received from the OAuth provider redirect.
    /// </summary>
    [JsonPropertyName("authCode")]
    public string AuthCode { get; init; } = string.Empty;

    /// <summary>
    /// The redirect URI that was used when initiating the OAuth flow.
    /// Must match the URI registered with the OAuth provider.
    /// </summary>
    [JsonPropertyName("redirectUri")]
    public string RedirectUri { get; init; } = string.Empty;
}

/// <summary>
/// Response DTO containing JWT access token, refresh token, and user info.
/// </summary>
public class TokenResponse
{
    [JsonPropertyName("accessToken")]
    public string AccessToken { get; init; } = string.Empty;

    [JsonPropertyName("refreshToken")]
    public string RefreshToken { get; init; } = string.Empty;

    [JsonPropertyName("expiresAt")]
    public DateTimeOffset ExpiresAt { get; init; }

    [JsonPropertyName("user")]
    public UserDto User { get; init; } = null!;
}

/// <summary>
/// Request DTO for refreshing an expired access token.
/// </summary>
public class RefreshRequest
{
    [JsonPropertyName("refreshToken")]
    public string RefreshToken { get; init; } = string.Empty;
}

/// <summary>
/// DTO representing user information returned to clients.
/// </summary>
public class UserDto
{
    [JsonPropertyName("id")]
    public Guid Id { get; init; }

    [JsonPropertyName("email")]
    public string Email { get; init; } = string.Empty;

    [JsonPropertyName("displayName")]
    public string DisplayName { get; init; } = string.Empty;

    [JsonPropertyName("role")]
    public string Role { get; init; } = string.Empty;

    [JsonPropertyName("providers")]
    public string[] Providers { get; init; } = [];
}

/// <summary>
/// Internal DTO representing the result of an OAuth token exchange.
/// </summary>
public class OAuthTokenResult
{
    public string AccessToken { get; init; } = string.Empty;
    public string? RefreshToken { get; init; }
    public string? IdToken { get; init; }
    public int ExpiresInSeconds { get; init; }
    public string Scopes { get; init; } = string.Empty;
}

/// <summary>
/// Internal DTO representing user profile information from an OAuth provider.
/// </summary>
public class OAuthUserInfo
{
    public string ProviderUserId { get; init; } = string.Empty;
    public string Email { get; init; } = string.Empty;
    public string DisplayName { get; init; } = string.Empty;
}
