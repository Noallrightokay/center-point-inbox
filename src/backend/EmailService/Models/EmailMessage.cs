namespace EmailService.Models;

public class EmailMessage
{
    public Guid Id { get; set; }
    public Guid AccountId { get; set; }
    public Guid TenantId { get; set; }
    public Guid UserId { get; set; }
    public required string MessageUid { get; set; }
    public required string Subject { get; set; }
    public string? Snippet { get; set; }
    public string? BodyHtml { get; set; }
    public string? BodyText { get; set; }
    public required string FromAddress { get; set; }
    public string? FromName { get; set; }
    public string[] ToAddresses { get; set; } = [];
    public string[] CcAddresses { get; set; } = [];
    public string[] BccAddresses { get; set; } = [];
    public string[] Labels { get; set; } = [];
    public bool IsRead { get; set; }
    public bool IsStarred { get; set; }
    public bool HasAttachments { get; set; }
    public string? ThreadId { get; set; }
    public string? InReplyTo { get; set; }
    public long? SizeBytes { get; set; }
    public DateTime ReceivedAt { get; set; }
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
}
