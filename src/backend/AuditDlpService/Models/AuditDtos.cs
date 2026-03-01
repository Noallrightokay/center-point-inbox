namespace CenterPointInbox.AuditDlpService.Models;

/// <summary>
/// DTO representing an individual audit log entry.
/// </summary>
public class AuditLogDto
{
    public Guid Id { get; init; }
    public Guid UserId { get; init; }
    public string Action { get; init; } = string.Empty;
    public string ResourceType { get; init; } = string.Empty;
    public string ResourceId { get; init; } = string.Empty;
    public string? IpAddress { get; init; }
    public Dictionary<string, object> Metadata { get; init; } = new();
    public DateTimeOffset CreatedAt { get; init; }
}

/// <summary>
/// Paginated response for audit log queries.
/// </summary>
public class AuditLogListResponse
{
    public List<AuditLogDto> Logs { get; init; } = new();
    public long TotalCount { get; init; }
    public int Page { get; init; }
    public int PageSize { get; init; }
}

/// <summary>
/// Query parameters for filtering and paginating audit logs.
/// </summary>
public class AuditQueryParams
{
    public int Page { get; set; } = 1;
    public int PageSize { get; set; } = 25;
    public Guid? UserId { get; set; }
    public string? Action { get; set; }
    public DateTimeOffset? StartDate { get; set; }
    public DateTimeOffset? EndDate { get; set; }
    public string? ResourceType { get; set; }
}

/// <summary>
/// DTO representing a DLP violation record.
/// </summary>
public class DlpViolationDto
{
    public Guid Id { get; init; }
    public Guid FileId { get; init; }
    public string ViolationType { get; init; } = string.Empty;
    public string Severity { get; init; } = string.Empty;
    public string Description { get; init; } = string.Empty;
    public string PatternMatched { get; init; } = string.Empty;
    public bool Resolved { get; init; }
    public DateTimeOffset CreatedAt { get; init; }
}

/// <summary>
/// Aggregated DLP dashboard response for admin overview.
/// </summary>
public class DlpDashboardResponse
{
    public long TotalViolations { get; init; }
    public long UnresolvedCount { get; init; }
    public Dictionary<string, long> BySeverity { get; init; } = new();
    public List<DlpViolationDto> RecentViolations { get; init; } = new();
}

/// <summary>
/// Aggregated admin dashboard response with platform-wide metrics.
/// </summary>
public class AdminDashboardResponse
{
    public long TotalUsers { get; init; }
    public long TotalFiles { get; init; }
    public long TotalTranslations { get; init; }
    public List<AuditLogDto> RecentActivity { get; init; } = new();
    public ViolationSummary ViolationSummary { get; init; } = new();
}

/// <summary>
/// Summary of DLP violation counts for the admin dashboard.
/// </summary>
public class ViolationSummary
{
    public long Total { get; init; }
    public long Unresolved { get; init; }
    public long Critical { get; init; }
    public long High { get; init; }
}
