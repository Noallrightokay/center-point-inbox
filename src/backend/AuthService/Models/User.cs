namespace CenterPointInbox.AuthService.Models;

/// <summary>
/// Represents a user in the Center Point Inbox platform.
/// </summary>
public class User
{
    public Guid Id { get; set; }

    public Guid TenantId { get; set; }

    public string Email { get; set; } = string.Empty;

    public string DisplayName { get; set; } = string.Empty;

    public Role Role { get; set; } = Role.User;

    /// <summary>
    /// Hashed refresh token for token rotation.
    /// </summary>
    public string? RefreshTokenHash { get; set; }

    /// <summary>
    /// Expiration time for the current refresh token.
    /// </summary>
    public DateTimeOffset? RefreshTokenExpiresAt { get; set; }

    public bool IsActive { get; set; } = true;

    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;

    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;

    public DateTimeOffset? LastLoginAt { get; set; }

    // Navigation properties
    public Tenant Tenant { get; set; } = null!;
    public ICollection<OAuthConnection> OAuthConnections { get; set; } = new List<OAuthConnection>();
}

/// <summary>
/// User roles within the platform.
/// </summary>
public enum Role
{
    User = 0,
    Admin = 1,
    Owner = 2
}
