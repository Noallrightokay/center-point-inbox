using System.Security.Claims;
using CenterPointInbox.AuthService.Models;
using CenterPointInbox.AuthService.Services;
using CenterPointInbox.Shared.Models;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

namespace CenterPointInbox.AuthService.Controllers;

/// <summary>
/// API controller for authentication endpoints: login, token refresh, token revocation,
/// and current user retrieval.
/// </summary>
[ApiController]
[Route("api/auth")]
public class AuthController : ControllerBase
{
    private readonly IAuthService _authService;
    private readonly ILogger<AuthController> _logger;

    public AuthController(IAuthService authService, ILogger<AuthController> logger)
    {
        _authService = authService;
        _logger = logger;
    }

    /// <summary>
    /// Authenticates a user via OAuth provider (Google or Microsoft).
    /// Exchanges the authorization code for tokens, creates/updates the user,
    /// and returns a JWT access token and refresh token.
    /// </summary>
    /// <param name="request">The login request containing provider, auth code, and redirect URI.</param>
    /// <returns>A token response with JWT, refresh token, expiration, and user info.</returns>
    [HttpPost("login")]
    [ProducesResponseType(typeof(ApiResponse<TokenResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status400BadRequest)]
    public async Task<IActionResult> Login([FromBody] LoginRequest request)
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;

        _logger.LogInformation(
            "Login attempt via {Provider} [CorrelationId: {CorrelationId}]",
            request.Provider, correlationId);

        var tokenResponse = await _authService.AuthenticateAsync(request);

        return Ok(ApiResponse<TokenResponse>.Success(tokenResponse, correlationId));
    }

    /// <summary>
    /// Refreshes an expired access token using a valid refresh token.
    /// The old refresh token is invalidated and a new one is issued (token rotation).
    /// </summary>
    /// <param name="request">The refresh request containing the current refresh token.</param>
    /// <returns>A new token response with fresh JWT and refresh token.</returns>
    [HttpPost("refresh")]
    [ProducesResponseType(typeof(ApiResponse<TokenResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    public async Task<IActionResult> Refresh([FromBody] RefreshRequest request)
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;

        _logger.LogDebug("Token refresh attempt [CorrelationId: {CorrelationId}]", correlationId);

        var tokenResponse = await _authService.RefreshTokenAsync(request.RefreshToken);

        return Ok(ApiResponse<TokenResponse>.Success(tokenResponse, correlationId));
    }

    /// <summary>
    /// Revokes the authenticated user's refresh token, effectively logging them out.
    /// Requires a valid JWT access token.
    /// </summary>
    /// <returns>A success response indicating the token was revoked.</returns>
    [HttpPost("revoke")]
    [Authorize]
    [ProducesResponseType(typeof(ApiResponse), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    public async Task<IActionResult> Revoke()
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userId = GetCurrentUserId();

        _logger.LogInformation(
            "Token revocation for user {UserId} [CorrelationId: {CorrelationId}]",
            userId, correlationId);

        await _authService.RevokeTokenAsync(userId);

        return Ok(ApiResponse.Success(correlationId));
    }

    /// <summary>
    /// Returns the profile information for the currently authenticated user.
    /// Requires a valid JWT access token.
    /// </summary>
    /// <returns>The current user's profile DTO.</returns>
    [HttpGet("me")]
    [Authorize]
    [ProducesResponseType(typeof(ApiResponse<UserDto>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> GetCurrentUser()
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userId = GetCurrentUserId();

        var userDto = await _authService.GetCurrentUserAsync(userId);

        return Ok(ApiResponse<UserDto>.Success(userDto, correlationId));
    }

    /// <summary>
    /// Extracts the authenticated user's ID from the JWT claims.
    /// </summary>
    private Guid GetCurrentUserId()
    {
        var userIdClaim = User.FindFirst(ClaimTypes.NameIdentifier)?.Value
            ?? User.FindFirst("sub")?.Value;

        if (string.IsNullOrEmpty(userIdClaim) || !Guid.TryParse(userIdClaim, out var userId))
        {
            throw new UnauthorizedAccessException("Missing or invalid user ID in token.");
        }

        return userId;
    }
}
