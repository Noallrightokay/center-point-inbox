namespace CenterPointInbox.MergerService.Models;

/// <summary>
/// Entity mapping to the translation_jobs table. Tracks document format
/// translation requests (e.g., DOCX -> PDF, Sheets -> CSV) including
/// PHI redaction tracking for compliance.
/// </summary>
public class TranslationJob
{
    public Guid Id { get; set; }
    public Guid UserId { get; set; }
    public Guid TenantId { get; set; }
    public Guid SourceFileId { get; set; }
    public string SourceFormat { get; set; } = string.Empty;
    public string TargetFormat { get; set; } = string.Empty;
    public string Status { get; set; } = "queued";
    public string? OutputStoragePath { get; set; }
    public string? ErrorMessage { get; set; }
    public bool PhiRedacted { get; set; }
    public DateTimeOffset? StartedAt { get; set; }
    public DateTimeOffset? CompletedAt { get; set; }
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;

    // Navigation property
    public FileMetadata? SourceFile { get; set; }
}
