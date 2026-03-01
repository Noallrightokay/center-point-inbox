namespace CenterPointInbox.AuditDlpService.Models;

/// <summary>
/// Simplified file metadata entity used for quarantine operations.
/// Maps to the "files_metadata" database table.
/// </summary>
public class FileMetadata
{
    public Guid Id { get; set; }

    public Guid TenantId { get; set; }

    public Guid UserId { get; set; }

    /// <summary>
    /// Original file name.
    /// </summary>
    public string FileName { get; set; } = string.Empty;

    /// <summary>
    /// MIME type of the file.
    /// </summary>
    public string ContentType { get; set; } = string.Empty;

    /// <summary>
    /// File size in bytes.
    /// </summary>
    public long SizeBytes { get; set; }

    /// <summary>
    /// Whether the file is currently quarantined due to DLP violations.
    /// </summary>
    public bool IsQuarantined { get; set; }

    /// <summary>
    /// The reason the file was quarantined.
    /// </summary>
    public string? QuarantineReason { get; set; }

    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;

    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;
}
