using CenterPointInbox.AuthService.Models;

namespace CenterPointInbox.AuthService.Services;

/// <summary>
/// Service responsible for exchanging OAuth authorization codes and retrieving user profiles
/// from Google and Microsoft identity providers.
/// </summary>
public interface IOAuthService
{
    /// <summary>
    /// Exchanges a Google OAuth authorization code for access and refresh tokens.
    /// </summary>
    /// <param name="code">The authorization code from Google's OAuth redirect.</param>
    /// <param name="redirectUri">The redirect URI used in the initial OAuth request.</param>
    /// <returns>The token exchange result containing access token, refresh token, and metadata.</returns>
    Task<OAuthTokenResult> ExchangeGoogleCodeAsync(string code, string redirectUri);

    /// <summary>
    /// Exchanges a Microsoft OAuth authorization code for access and refresh tokens.
    /// </summary>
    /// <param name="code">The authorization code from Microsoft's OAuth redirect.</param>
    /// <param name="redirectUri">The redirect URI used in the initial OAuth request.</param>
    /// <returns>The token exchange result containing access token, refresh token, and metadata.</returns>
    Task<OAuthTokenResult> ExchangeMicrosoftCodeAsync(string code, string redirectUri);

    /// <summary>
    /// Retrieves the authenticated user's profile from Google using an access token.
    /// </summary>
    /// <param name="accessToken">A valid Google OAuth access token.</param>
    /// <returns>The user's profile information from Google.</returns>
    Task<OAuthUserInfo> GetGoogleUserInfoAsync(string accessToken);

    /// <summary>
    /// Retrieves the authenticated user's profile from Microsoft Graph using an access token.
    /// </summary>
    /// <param name="accessToken">A valid Microsoft OAuth access token.</param>
    /// <returns>The user's profile information from Microsoft.</returns>
    Task<OAuthUserInfo> GetMicrosoftUserInfoAsync(string accessToken);
}
