using CenterPointInbox.AuthService.Models;

namespace CenterPointInbox.AuthService.Services;

/// <summary>
/// Core authentication service responsible for user authentication, token management,
/// and user profile retrieval.
/// </summary>
public interface IAuthService
{
    /// <summary>
    /// Authenticates a user by exchanging an OAuth authorization code for provider tokens,
    /// creating or updating the user in the database, and issuing a JWT access token and refresh token.
    /// </summary>
    /// <param name="request">The login request containing the OAuth provider, auth code, and redirect URI.</param>
    /// <returns>A token response containing the JWT, refresh token, expiration, and user info.</returns>
    Task<TokenResponse> AuthenticateAsync(LoginRequest request);

    /// <summary>
    /// Refreshes an expired access token using a valid refresh token.
    /// Implements token rotation: the old refresh token is invalidated and a new one is issued.
    /// </summary>
    /// <param name="refreshToken">The current refresh token.</param>
    /// <returns>A new token response with fresh JWT and refresh token.</returns>
    Task<TokenResponse> RefreshTokenAsync(string refreshToken);

    /// <summary>
    /// Revokes all tokens for a given user, effectively logging them out.
    /// </summary>
    /// <param name="userId">The ID of the user whose tokens should be revoked.</param>
    Task RevokeTokenAsync(Guid userId);

    /// <summary>
    /// Retrieves the current user's profile information.
    /// </summary>
    /// <param name="userId">The ID of the authenticated user.</param>
    /// <returns>The user's profile DTO.</returns>
    Task<UserDto> GetCurrentUserAsync(Guid userId);
}
