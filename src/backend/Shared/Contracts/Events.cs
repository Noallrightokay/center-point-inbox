namespace CenterPointInbox.Shared.Contracts;

/// <summary>
/// Published when a file needs to be indexed for search.
/// Consumed by the Indexing Service.
/// </summary>
public record FileIndexRequested
{
    public Guid TenantId { get; init; }
    public Guid UserId { get; init; }
    public Guid FileId { get; init; }
    public string Provider { get; init; } = string.Empty;
    public DateTimeOffset RequestedAt { get; init; } = DateTimeOffset.UtcNow;
}

/// <summary>
/// Published when a translation/conversion job is requested.
/// Consumed by the Translation Service.
/// </summary>
public record TranslationRequested
{
    public Guid TenantId { get; init; }
    public Guid UserId { get; init; }
    public Guid SourceFileId { get; init; }
    public string SourceFormat { get; init; } = string.Empty;
    public string TargetFormat { get; init; } = string.Empty;
    public bool RedactPhi { get; init; }
    public DateTimeOffset RequestedAt { get; init; } = DateTimeOffset.UtcNow;
}

/// <summary>
/// Published when a translation job completes (success or failure).
/// Consumed by services awaiting translation results.
/// </summary>
public record TranslationCompleted
{
    public Guid JobId { get; init; }
    public TranslationStatus Status { get; init; }
    public string? OutputPath { get; init; }
    public string? Error { get; init; }
    public DateTimeOffset CompletedAt { get; init; } = DateTimeOffset.UtcNow;
}

/// <summary>
/// Published when content needs DLP (Data Loss Prevention) scanning.
/// Consumed by the DLP Service.
/// </summary>
public record DlpScanRequested
{
    public Guid TenantId { get; init; }
    public Guid FileId { get; init; }
    public string Content { get; init; } = string.Empty;
    public DateTimeOffset RequestedAt { get; init; } = DateTimeOffset.UtcNow;
}

/// <summary>
/// Published when a DLP violation is detected during scanning.
/// Consumed by notification/audit services.
/// </summary>
public record DlpViolationDetected
{
    public Guid TenantId { get; init; }
    public Guid FileId { get; init; }
    public string ViolationType { get; init; } = string.Empty;
    public DlpSeverity Severity { get; init; }
    public string Pattern { get; init; } = string.Empty;
    public DateTimeOffset DetectedAt { get; init; } = DateTimeOffset.UtcNow;
}

/// <summary>
/// Published for every auditable action in the system.
/// Consumed by the Audit Service.
/// </summary>
public record AuditEvent
{
    public Guid TenantId { get; init; }
    public Guid UserId { get; init; }
    public string Action { get; init; } = string.Empty;
    public string ResourceType { get; init; } = string.Empty;
    public string ResourceId { get; init; } = string.Empty;
    public string? IpAddress { get; init; }
    public IDictionary<string, string>? Metadata { get; init; }
    public DateTimeOffset OccurredAt { get; init; } = DateTimeOffset.UtcNow;
}

public enum TranslationStatus
{
    Completed,
    Failed,
    PartiallyCompleted
}

public enum DlpSeverity
{
    Low,
    Medium,
    High,
    Critical
}
