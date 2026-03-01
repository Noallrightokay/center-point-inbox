using System.Security.Claims;

namespace CenterPointInbox.Shared.Models;

public class UserContext
{
    public Guid UserId { get; init; }
    public Guid TenantId { get; init; }
    public string Email { get; init; } = string.Empty;
    public string Role { get; init; } = string.Empty;
    public IReadOnlyList<ProviderScope> ProviderScopes { get; init; } = [];

    public bool HasScope(string provider, string scope)
    {
        return ProviderScopes.Any(ps =>
            ps.Provider.Equals(provider, StringComparison.OrdinalIgnoreCase) &&
            ps.Scopes.Contains(scope, StringComparer.OrdinalIgnoreCase));
    }

    public bool IsAdmin => Role.Equals("Admin", StringComparison.OrdinalIgnoreCase);

    public static UserContext FromClaimsPrincipal(ClaimsPrincipal principal)
    {
        var userIdClaim = principal.FindFirst(ClaimTypes.NameIdentifier)?.Value
            ?? principal.FindFirst("sub")?.Value;
        var tenantIdClaim = principal.FindFirst("tenant_id")?.Value;
        var emailClaim = principal.FindFirst(ClaimTypes.Email)?.Value
            ?? principal.FindFirst("email")?.Value;
        var roleClaim = principal.FindFirst(ClaimTypes.Role)?.Value
            ?? principal.FindFirst("role")?.Value;
        var scopesClaim = principal.FindFirst("provider_scopes")?.Value;

        if (string.IsNullOrEmpty(userIdClaim) || !Guid.TryParse(userIdClaim, out var userId))
        {
            throw new UnauthorizedAccessException("Missing or invalid user ID claim.");
        }

        if (string.IsNullOrEmpty(tenantIdClaim) || !Guid.TryParse(tenantIdClaim, out var tenantId))
        {
            throw new UnauthorizedAccessException("Missing or invalid tenant ID claim.");
        }

        var providerScopes = ParseProviderScopes(scopesClaim);

        return new UserContext
        {
            UserId = userId,
            TenantId = tenantId,
            Email = emailClaim ?? string.Empty,
            Role = roleClaim ?? "User",
            ProviderScopes = providerScopes
        };
    }

    /// <summary>
    /// Parses provider scopes from a claim value.
    /// Expected format: "gmail:read,send;outlook:read,send,calendar"
    /// </summary>
    private static List<ProviderScope> ParseProviderScopes(string? scopesClaim)
    {
        if (string.IsNullOrWhiteSpace(scopesClaim))
        {
            return [];
        }

        var result = new List<ProviderScope>();
        var providers = scopesClaim.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);

        foreach (var providerEntry in providers)
        {
            var parts = providerEntry.Split(':', 2, StringSplitOptions.TrimEntries);
            if (parts.Length != 2)
            {
                continue;
            }

            var provider = parts[0];
            var scopes = parts[1]
                .Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                .ToList();

            result.Add(new ProviderScope
            {
                Provider = provider,
                Scopes = scopes
            });
        }

        return result;
    }
}

public class ProviderScope
{
    public string Provider { get; init; } = string.Empty;
    public IReadOnlyList<string> Scopes { get; init; } = [];
}
