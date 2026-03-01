namespace CenterPointInbox.AuthService.Models;

/// <summary>
/// Represents a tenant (organization) in the multi-tenant platform.
/// </summary>
public class Tenant
{
    public Guid Id { get; set; }

    public string Name { get; set; } = string.Empty;

    public string? Domain { get; set; }

    public bool IsActive { get; set; } = true;

    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;

    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;

    // Navigation properties
    public ICollection<User> Users { get; set; } = new List<User>();
}
