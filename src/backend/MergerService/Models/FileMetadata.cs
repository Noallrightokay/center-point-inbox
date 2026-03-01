namespace CenterPointInbox.MergerService.Models;

/// <summary>
/// Entity mapping to the files_metadata table. Represents the canonical metadata
/// record for a document synced from Google Drive or OneDrive/SharePoint.
/// </summary>
public class FileMetadata
{
    public Guid Id { get; set; }
    public Guid UserId { get; set; }
    public Guid TenantId { get; set; }
    public string Provider { get; set; } = string.Empty;
    public string ProviderFileId { get; set; } = string.Empty;
    public string FileName { get; set; } = string.Empty;
    public string MimeType { get; set; } = string.Empty;
    public long? FileSizeBytes { get; set; }
    public string? ParentFolderId { get; set; }
    public string? ProviderUrl { get; set; }
    public DateTimeOffset? LastModifiedProvider { get; set; }
    public bool IsShared { get; set; }
    public string PermissionsJson { get; set; } = "[]";
    public bool PhiDetected { get; set; }
    public bool Quarantined { get; set; }
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;
}
