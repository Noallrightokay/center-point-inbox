namespace CenterPointInbox.AiAssistantService.Models;

/// <summary>
/// Simplified entity for ACL checking. Maps to the shared files_metadata table
/// but only includes the columns needed for access control decisions.
/// </summary>
public class FileMetadata
{
    public Guid Id { get; set; }
    public Guid UserId { get; set; }
    public Guid TenantId { get; set; }
    public string Provider { get; set; } = string.Empty;
    public string FileName { get; set; } = string.Empty;
    public bool Quarantined { get; set; }
    public bool IsShared { get; set; }
}
