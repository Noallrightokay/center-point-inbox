namespace CenterPointInbox.AuditDlpService.Models;

/// <summary>
/// Represents a Data Loss Prevention violation detected during content scanning.
/// Maps to the "dlp_violations" database table.
/// </summary>
public class DlpViolation
{
    public Guid Id { get; set; }

    public Guid TenantId { get; set; }

    /// <summary>
    /// The file that triggered the violation.
    /// </summary>
    public Guid FileId { get; set; }

    /// <summary>
    /// The type of violation detected (e.g., "PHI", "SSN", "MRN").
    /// </summary>
    public string ViolationType { get; set; } = string.Empty;

    /// <summary>
    /// The severity level of the violation.
    /// </summary>
    public Severity Severity { get; set; } = Severity.Medium;

    /// <summary>
    /// Human-readable description of the violation.
    /// </summary>
    public string Description { get; set; } = string.Empty;

    /// <summary>
    /// The regex pattern that matched the content.
    /// </summary>
    public string PatternMatched { get; set; } = string.Empty;

    /// <summary>
    /// Whether the violation has been reviewed and resolved by an admin.
    /// </summary>
    public bool Resolved { get; set; }

    /// <summary>
    /// The user who resolved the violation, if applicable.
    /// </summary>
    public Guid? ResolvedByUserId { get; set; }

    /// <summary>
    /// When the violation was resolved.
    /// </summary>
    public DateTimeOffset? ResolvedAt { get; set; }

    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
}

/// <summary>
/// Severity levels for DLP violations.
/// </summary>
public enum Severity
{
    Low = 0,
    Medium = 1,
    High = 2,
    Critical = 3
}
