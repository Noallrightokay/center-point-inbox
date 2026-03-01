using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Security.Cryptography;
using System.Text;
using CenterPointInbox.AuthService.Models;
using CenterPointInbox.Shared.Security;
using Microsoft.Extensions.Options;
using Microsoft.IdentityModel.Tokens;

namespace CenterPointInbox.AuthService.Services;

/// <summary>
/// Implementation of ITokenService that generates JWT access tokens with HMAC-SHA256 signing
/// and cryptographically secure refresh tokens.
/// </summary>
public class TokenService : ITokenService
{
    private readonly JwtConfig _jwtConfig;
    private readonly SigningCredentials _signingCredentials;

    public TokenService(IOptions<JwtConfig> jwtConfig)
    {
        _jwtConfig = jwtConfig.Value;

        var keyBytes = Encoding.UTF8.GetBytes(_jwtConfig.SigningKey);
        var securityKey = new SymmetricSecurityKey(keyBytes);
        _signingCredentials = new SigningCredentials(securityKey, SecurityAlgorithms.HmacSha256);
    }

    /// <inheritdoc />
    public string GenerateJwt(User user, Tenant tenant)
    {
        ArgumentNullException.ThrowIfNull(user);
        ArgumentNullException.ThrowIfNull(tenant);

        var providers = user.OAuthConnections
            .Select(c => c.Provider.ToString())
            .Distinct()
            .ToArray();

        var claims = new List<Claim>
        {
            new(JwtRegisteredClaimNames.Sub, user.Id.ToString()),
            new(JwtRegisteredClaimNames.Email, user.Email),
            new(JwtRegisteredClaimNames.Jti, Guid.NewGuid().ToString()),
            new("tenant_id", tenant.Id.ToString()),
            new(ClaimTypes.Role, user.Role.ToString()),
        };

        // Add each provider as a separate claim so they appear as an array in the JWT
        foreach (var provider in providers)
        {
            claims.Add(new Claim("providers", provider));
        }

        var expiresAt = DateTime.UtcNow.AddMinutes(_jwtConfig.ExpirationMinutes);

        var tokenDescriptor = new SecurityTokenDescriptor
        {
            Subject = new ClaimsIdentity(claims),
            Expires = expiresAt,
            Issuer = _jwtConfig.Issuer,
            Audience = _jwtConfig.Audience,
            SigningCredentials = _signingCredentials,
            IssuedAt = DateTime.UtcNow,
            NotBefore = DateTime.UtcNow
        };

        var tokenHandler = new JwtSecurityTokenHandler();
        var securityToken = tokenHandler.CreateToken(tokenDescriptor);

        return tokenHandler.WriteToken(securityToken);
    }

    /// <inheritdoc />
    public string GenerateRefreshToken()
    {
        var randomBytes = new byte[64];
        RandomNumberGenerator.Fill(randomBytes);
        return Convert.ToBase64String(randomBytes);
    }

    /// <inheritdoc />
    public ClaimsPrincipal? ValidateRefreshToken(string token)
    {
        if (string.IsNullOrWhiteSpace(token))
        {
            return null;
        }

        // Refresh tokens are opaque random strings, not JWTs.
        // Validation is done by looking up the hashed token in the database.
        // This method is provided for interface compatibility if JWT-based refresh tokens
        // are used in the future. For the current opaque token scheme, the AuthService
        // validates the hash directly against the database.
        //
        // Return a minimal ClaimsPrincipal to indicate the token format is valid.
        try
        {
            // Verify the token is valid base64
            var bytes = Convert.FromBase64String(token);
            if (bytes.Length != 64)
            {
                return null;
            }

            // Return a generic principal — actual user lookup is done in AuthService
            var identity = new ClaimsIdentity("RefreshToken");
            return new ClaimsPrincipal(identity);
        }
        catch (FormatException)
        {
            return null;
        }
    }
}
