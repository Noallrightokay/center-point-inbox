using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace CenterPointInbox.TranslationWorker.Models;

/// <summary>
/// Represents the processing status of a translation job.
/// </summary>
public enum TranslationJobStatus
{
    Queued = 0,
    Processing = 1,
    Completed = 2,
    Failed = 3
}

/// <summary>
/// Entity mapping to the translation_jobs table.
/// Tracks the lifecycle of a document format conversion request.
/// </summary>
[Table("translation_jobs")]
public class TranslationJob
{
    [Key]
    [Column("id")]
    public Guid Id { get; set; }

    [Required]
    [Column("tenant_id")]
    public Guid TenantId { get; set; }

    [Required]
    [Column("user_id")]
    public Guid UserId { get; set; }

    [Required]
    [Column("source_file_id")]
    public Guid SourceFileId { get; set; }

    [Required]
    [Column("source_format")]
    [MaxLength(256)]
    public string SourceFormat { get; set; } = string.Empty;

    [Required]
    [Column("target_format")]
    [MaxLength(256)]
    public string TargetFormat { get; set; } = string.Empty;

    [Required]
    [Column("status")]
    [MaxLength(50)]
    public TranslationJobStatus Status { get; set; } = TranslationJobStatus.Queued;

    [Column("redact_phi")]
    public bool RedactPhi { get; set; }

    [Column("output_path")]
    [MaxLength(1024)]
    public string? OutputPath { get; set; }

    [Column("error_message")]
    [MaxLength(4000)]
    public string? ErrorMessage { get; set; }

    [Column("source_file_size_bytes")]
    public long? SourceFileSizeBytes { get; set; }

    [Column("output_file_size_bytes")]
    public long? OutputFileSizeBytes { get; set; }

    [Column("created_at")]
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;

    [Column("started_at")]
    public DateTimeOffset? StartedAt { get; set; }

    [Column("completed_at")]
    public DateTimeOffset? CompletedAt { get; set; }

    [Column("updated_at")]
    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;
}

/// <summary>
/// Entity mapping to the files_metadata table.
/// Stores metadata about files managed by the platform.
/// </summary>
[Table("files_metadata")]
public class FileMetadata
{
    [Key]
    [Column("id")]
    public Guid Id { get; set; }

    [Required]
    [Column("tenant_id")]
    public Guid TenantId { get; set; }

    [Required]
    [Column("user_id")]
    public Guid UserId { get; set; }

    [Required]
    [Column("file_name")]
    [MaxLength(512)]
    public string FileName { get; set; } = string.Empty;

    [Required]
    [Column("content_type")]
    [MaxLength(256)]
    public string ContentType { get; set; } = string.Empty;

    [Column("file_size_bytes")]
    public long FileSizeBytes { get; set; }

    [Required]
    [Column("provider")]
    [MaxLength(50)]
    public string Provider { get; set; } = string.Empty;

    [Column("provider_file_id")]
    [MaxLength(512)]
    public string? ProviderFileId { get; set; }

    [Column("storage_path")]
    [MaxLength(1024)]
    public string? StoragePath { get; set; }

    [Column("created_at")]
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;

    [Column("updated_at")]
    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;
}
