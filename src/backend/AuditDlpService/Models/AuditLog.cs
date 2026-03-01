namespace CenterPointInbox.AuditDlpService.Models;

/// <summary>
/// Represents an auditable action recorded in the system.
/// Maps to the "audit_logs" database table.
/// </summary>
public class AuditLog
{
    public Guid Id { get; set; }

    public Guid TenantId { get; set; }

    public Guid UserId { get; set; }

    /// <summary>
    /// The action performed (e.g., "file.download", "user.login", "translation.requested").
    /// </summary>
    public string Action { get; set; } = string.Empty;

    /// <summary>
    /// The type of resource affected (e.g., "file", "user", "translation").
    /// </summary>
    public string ResourceType { get; set; } = string.Empty;

    /// <summary>
    /// The identifier of the affected resource.
    /// </summary>
    public string ResourceId { get; set; } = string.Empty;

    /// <summary>
    /// The IP address from which the action originated.
    /// </summary>
    public string? IpAddress { get; set; }

    /// <summary>
    /// The user agent string of the client that performed the action.
    /// </summary>
    public string? UserAgent { get; set; }

    /// <summary>
    /// Additional metadata associated with the audit event, stored as JSONB.
    /// </summary>
    public Dictionary<string, object> Metadata { get; set; } = new();

    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
}
