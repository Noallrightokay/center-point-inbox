using System.Security.Claims;
using CenterPointInbox.AuthService.Models;

namespace CenterPointInbox.AuthService.Services;

/// <summary>
/// Service responsible for generating and validating JWT access tokens and refresh tokens.
/// </summary>
public interface ITokenService
{
    /// <summary>
    /// Generates a signed JWT access token containing user claims.
    /// </summary>
    /// <param name="user">The authenticated user entity.</param>
    /// <param name="tenant">The user's tenant entity.</param>
    /// <returns>A signed JWT string.</returns>
    string GenerateJwt(User user, Tenant tenant);

    /// <summary>
    /// Generates a cryptographically secure random refresh token.
    /// </summary>
    /// <returns>A base64-encoded refresh token string.</returns>
    string GenerateRefreshToken();

    /// <summary>
    /// Validates a refresh token and extracts the claims principal.
    /// Returns null if the token is invalid or expired.
    /// </summary>
    /// <param name="token">The refresh token to validate.</param>
    /// <returns>A ClaimsPrincipal if valid; otherwise null.</returns>
    ClaimsPrincipal? ValidateRefreshToken(string token);
}
