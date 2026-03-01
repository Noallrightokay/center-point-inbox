using System.Security.Authentication;
using System.Security.Cryptography;
using System.Text;
using CenterPointInbox.AuthService.Configuration;
using CenterPointInbox.AuthService.Models;
using CenterPointInbox.Shared.Contracts;
using CenterPointInbox.Shared.Security;
using MassTransit;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace CenterPointInbox.AuthService.Services;

/// <summary>
/// Implementation of IAuthService that orchestrates OAuth authentication, user management,
/// JWT issuance, and audit event publishing.
/// </summary>
public class AuthServiceImpl : IAuthService
{
    private readonly AuthDbContext _dbContext;
    private readonly IOAuthService _oAuthService;
    private readonly ITokenService _tokenService;
    private readonly IPublishEndpoint _publishEndpoint;
    private readonly AuthSettings _authSettings;
    private readonly JwtConfig _jwtConfig;
    private readonly ILogger<AuthServiceImpl> _logger;

    public AuthServiceImpl(
        AuthDbContext dbContext,
        IOAuthService oAuthService,
        ITokenService tokenService,
        IPublishEndpoint publishEndpoint,
        IOptions<AuthSettings> authSettings,
        IOptions<JwtConfig> jwtConfig,
        ILogger<AuthServiceImpl> logger)
    {
        _dbContext = dbContext;
        _oAuthService = oAuthService;
        _tokenService = tokenService;
        _publishEndpoint = publishEndpoint;
        _authSettings = authSettings.Value;
        _jwtConfig = jwtConfig.Value;
        _logger = logger;
    }

    /// <inheritdoc />
    public async Task<TokenResponse> AuthenticateAsync(LoginRequest request)
    {
        ArgumentNullException.ThrowIfNull(request);
        ArgumentException.ThrowIfNullOrEmpty(request.AuthCode);
        ArgumentException.ThrowIfNullOrEmpty(request.RedirectUri);

        _logger.LogInformation("Authenticating user via {Provider}", request.Provider);

        // Step 1: Exchange the auth code for OAuth tokens
        var oAuthTokens = request.Provider switch
        {
            Provider.Google => await _oAuthService.ExchangeGoogleCodeAsync(request.AuthCode, request.RedirectUri),
            Provider.Microsoft => await _oAuthService.ExchangeMicrosoftCodeAsync(request.AuthCode, request.RedirectUri),
            _ => throw new ArgumentException($"Unsupported OAuth provider: {request.Provider}")
        };

        // Step 2: Retrieve user profile from the OAuth provider
        var userInfo = request.Provider switch
        {
            Provider.Google => await _oAuthService.GetGoogleUserInfoAsync(oAuthTokens.AccessToken),
            Provider.Microsoft => await _oAuthService.GetMicrosoftUserInfoAsync(oAuthTokens.AccessToken),
            _ => throw new ArgumentException($"Unsupported OAuth provider: {request.Provider}")
        };

        // Step 3: Create or update user and OAuth connection in the database
        var user = await CreateOrUpdateUserAsync(userInfo, request.Provider, oAuthTokens);

        // Step 4: Load the tenant for JWT claims
        var tenant = await _dbContext.Tenants.FindAsync(user.TenantId)
            ?? throw new InvalidOperationException($"Tenant {user.TenantId} not found for user {user.Id}.");

        // Step 5: Generate JWT and refresh token
        var jwt = _tokenService.GenerateJwt(user, tenant);
        var refreshToken = _tokenService.GenerateRefreshToken();
        var expiresAt = DateTimeOffset.UtcNow.AddMinutes(_jwtConfig.ExpirationMinutes);

        // Step 6: Store hashed refresh token on the user
        user.RefreshTokenHash = HashToken(refreshToken);
        user.RefreshTokenExpiresAt = DateTimeOffset.UtcNow.AddDays(_jwtConfig.RefreshExpirationDays);
        user.LastLoginAt = DateTimeOffset.UtcNow;
        user.UpdatedAt = DateTimeOffset.UtcNow;

        await _dbContext.SaveChangesAsync();

        _logger.LogInformation(
            "User {UserId} authenticated successfully via {Provider}",
            user.Id, request.Provider);

        // Step 7: Publish audit event for login
        await _publishEndpoint.Publish(new AuditEvent
        {
            TenantId = tenant.Id,
            UserId = user.Id,
            Action = "UserLogin",
            ResourceType = "User",
            ResourceId = user.Id.ToString(),
            Metadata = new Dictionary<string, string>
            {
                ["provider"] = request.Provider.ToString(),
                ["email"] = user.Email
            },
            OccurredAt = DateTimeOffset.UtcNow
        });

        return new TokenResponse
        {
            AccessToken = jwt,
            RefreshToken = refreshToken,
            ExpiresAt = expiresAt,
            User = MapToUserDto(user)
        };
    }

    /// <inheritdoc />
    public async Task<TokenResponse> RefreshTokenAsync(string refreshToken)
    {
        ArgumentException.ThrowIfNullOrEmpty(refreshToken);

        // Validate the token format
        var principal = _tokenService.ValidateRefreshToken(refreshToken);
        if (principal is null)
        {
            throw new AuthenticationException("Invalid refresh token format.");
        }

        // Find the user by refresh token hash
        var tokenHash = HashToken(refreshToken);
        var user = await _dbContext.Users
            .Include(u => u.OAuthConnections)
            .FirstOrDefaultAsync(u => u.RefreshTokenHash == tokenHash);

        if (user is null)
        {
            _logger.LogWarning("Refresh token not found in database — possible token reuse");
            throw new AuthenticationException("Invalid refresh token.");
        }

        if (!user.IsActive)
        {
            _logger.LogWarning("Refresh token used by deactivated user {UserId}", user.Id);
            throw new AuthenticationException("User account is deactivated.");
        }

        if (user.RefreshTokenExpiresAt.HasValue && user.RefreshTokenExpiresAt.Value < DateTimeOffset.UtcNow)
        {
            _logger.LogWarning("Expired refresh token used by user {UserId}", user.Id);
            // Clear the expired token
            user.RefreshTokenHash = null;
            user.RefreshTokenExpiresAt = null;
            await _dbContext.SaveChangesAsync();
            throw new AuthenticationException("Refresh token has expired. Please log in again.");
        }

        var tenant = await _dbContext.Tenants.FindAsync(user.TenantId)
            ?? throw new InvalidOperationException($"Tenant {user.TenantId} not found for user {user.Id}.");

        // Generate new JWT and refresh token (token rotation)
        var newJwt = _tokenService.GenerateJwt(user, tenant);
        var newRefreshToken = _tokenService.GenerateRefreshToken();
        var expiresAt = DateTimeOffset.UtcNow.AddMinutes(_jwtConfig.ExpirationMinutes);

        // Update the stored refresh token hash
        user.RefreshTokenHash = HashToken(newRefreshToken);
        user.RefreshTokenExpiresAt = DateTimeOffset.UtcNow.AddDays(_jwtConfig.RefreshExpirationDays);
        user.UpdatedAt = DateTimeOffset.UtcNow;

        await _dbContext.SaveChangesAsync();

        _logger.LogInformation("Tokens refreshed for user {UserId}", user.Id);

        return new TokenResponse
        {
            AccessToken = newJwt,
            RefreshToken = newRefreshToken,
            ExpiresAt = expiresAt,
            User = MapToUserDto(user)
        };
    }

    /// <inheritdoc />
    public async Task RevokeTokenAsync(Guid userId)
    {
        var user = await _dbContext.Users.FindAsync(userId);

        if (user is null)
        {
            _logger.LogWarning("Attempted to revoke tokens for non-existent user {UserId}", userId);
            throw new KeyNotFoundException($"User {userId} not found.");
        }

        user.RefreshTokenHash = null;
        user.RefreshTokenExpiresAt = null;
        user.UpdatedAt = DateTimeOffset.UtcNow;

        await _dbContext.SaveChangesAsync();

        _logger.LogInformation("Tokens revoked for user {UserId}", userId);

        await _publishEndpoint.Publish(new AuditEvent
        {
            TenantId = user.TenantId,
            UserId = user.Id,
            Action = "UserLogout",
            ResourceType = "User",
            ResourceId = user.Id.ToString(),
            OccurredAt = DateTimeOffset.UtcNow
        });
    }

    /// <inheritdoc />
    public async Task<UserDto> GetCurrentUserAsync(Guid userId)
    {
        var user = await _dbContext.Users
            .Include(u => u.OAuthConnections)
            .FirstOrDefaultAsync(u => u.Id == userId);

        if (user is null)
        {
            throw new KeyNotFoundException($"User {userId} not found.");
        }

        return MapToUserDto(user);
    }

    /// <summary>
    /// Creates a new user or updates an existing one based on the OAuth provider user info.
    /// Also creates or updates the corresponding OAuthConnection with encrypted tokens.
    /// </summary>
    private async Task<User> CreateOrUpdateUserAsync(
        OAuthUserInfo userInfo,
        Provider provider,
        OAuthTokenResult oAuthTokens)
    {
        // Try to find an existing OAuth connection for this provider + provider user ID
        var existingConnection = await _dbContext.OAuthConnections
            .Include(c => c.User)
                .ThenInclude(u => u.OAuthConnections)
            .FirstOrDefaultAsync(c =>
                c.Provider == provider &&
                c.ProviderUserId == userInfo.ProviderUserId);

        User user;

        if (existingConnection is not null)
        {
            // Existing user — update their OAuth tokens
            user = existingConnection.User;

            existingConnection.EncryptedAccessToken = TokenEncryption.Encrypt(
                oAuthTokens.AccessToken, _authSettings.TokenEncryptionKey);

            if (!string.IsNullOrEmpty(oAuthTokens.RefreshToken))
            {
                existingConnection.EncryptedRefreshToken = TokenEncryption.Encrypt(
                    oAuthTokens.RefreshToken, _authSettings.TokenEncryptionKey);
            }

            existingConnection.AccessTokenExpiresAt = oAuthTokens.ExpiresInSeconds > 0
                ? DateTimeOffset.UtcNow.AddSeconds(oAuthTokens.ExpiresInSeconds)
                : null;

            existingConnection.Scopes = oAuthTokens.Scopes;
            existingConnection.ProviderEmail = userInfo.Email;
            existingConnection.UpdatedAt = DateTimeOffset.UtcNow;

            // Update user display name if changed
            if (!string.IsNullOrEmpty(userInfo.DisplayName))
            {
                user.DisplayName = userInfo.DisplayName;
            }

            user.UpdatedAt = DateTimeOffset.UtcNow;

            _logger.LogDebug(
                "Updated OAuth connection for user {UserId} with provider {Provider}",
                user.Id, provider);
        }
        else
        {
            // Check if a user with this email already exists (linking another provider)
            user = await _dbContext.Users
                .Include(u => u.OAuthConnections)
                .FirstOrDefaultAsync(u => u.Email == userInfo.Email);

            if (user is null)
            {
                // Brand new user — create tenant and user
                var tenant = new Tenant
                {
                    Id = Guid.NewGuid(),
                    Name = ExtractDomainFromEmail(userInfo.Email),
                    Domain = ExtractDomainFromEmail(userInfo.Email),
                    IsActive = true,
                    CreatedAt = DateTimeOffset.UtcNow,
                    UpdatedAt = DateTimeOffset.UtcNow
                };

                // Check if a tenant with this domain already exists
                var existingTenant = await _dbContext.Tenants
                    .FirstOrDefaultAsync(t => t.Domain == tenant.Domain);

                if (existingTenant is not null)
                {
                    tenant = existingTenant;
                }
                else
                {
                    _dbContext.Tenants.Add(tenant);
                }

                user = new User
                {
                    Id = Guid.NewGuid(),
                    TenantId = tenant.Id,
                    Email = userInfo.Email,
                    DisplayName = userInfo.DisplayName,
                    Role = Role.User,
                    IsActive = true,
                    CreatedAt = DateTimeOffset.UtcNow,
                    UpdatedAt = DateTimeOffset.UtcNow
                };

                _dbContext.Users.Add(user);

                _logger.LogInformation(
                    "Created new user {UserId} with email {Email} in tenant {TenantId}",
                    user.Id, user.Email, tenant.Id);
            }

            // Create the OAuth connection
            var connection = new OAuthConnection
            {
                Id = Guid.NewGuid(),
                UserId = user.Id,
                Provider = provider,
                ProviderUserId = userInfo.ProviderUserId,
                ProviderEmail = userInfo.Email,
                EncryptedAccessToken = TokenEncryption.Encrypt(
                    oAuthTokens.AccessToken, _authSettings.TokenEncryptionKey),
                EncryptedRefreshToken = !string.IsNullOrEmpty(oAuthTokens.RefreshToken)
                    ? TokenEncryption.Encrypt(oAuthTokens.RefreshToken, _authSettings.TokenEncryptionKey)
                    : null,
                AccessTokenExpiresAt = oAuthTokens.ExpiresInSeconds > 0
                    ? DateTimeOffset.UtcNow.AddSeconds(oAuthTokens.ExpiresInSeconds)
                    : null,
                Scopes = oAuthTokens.Scopes,
                CreatedAt = DateTimeOffset.UtcNow,
                UpdatedAt = DateTimeOffset.UtcNow
            };

            _dbContext.OAuthConnections.Add(connection);
            user.OAuthConnections.Add(connection);

            _logger.LogDebug(
                "Created OAuth connection for user {UserId} with provider {Provider}",
                user.Id, provider);
        }

        await _dbContext.SaveChangesAsync();

        // Reload the user with all connections for JWT generation
        user = await _dbContext.Users
            .Include(u => u.OAuthConnections)
            .FirstAsync(u => u.Id == user.Id);

        return user;
    }

    /// <summary>
    /// Maps a User entity to a UserDto.
    /// </summary>
    private static UserDto MapToUserDto(User user)
    {
        return new UserDto
        {
            Id = user.Id,
            Email = user.Email,
            DisplayName = user.DisplayName,
            Role = user.Role.ToString(),
            Providers = user.OAuthConnections
                .Select(c => c.Provider.ToString())
                .Distinct()
                .ToArray()
        };
    }

    /// <summary>
    /// Hashes a refresh token using SHA-256 for secure storage.
    /// </summary>
    private static string HashToken(string token)
    {
        var bytes = Encoding.UTF8.GetBytes(token);
        var hash = SHA256.HashData(bytes);
        return Convert.ToBase64String(hash);
    }

    /// <summary>
    /// Extracts the domain portion from an email address.
    /// </summary>
    private static string ExtractDomainFromEmail(string email)
    {
        var atIndex = email.IndexOf('@');
        return atIndex >= 0 ? email[(atIndex + 1)..] : email;
    }
}
