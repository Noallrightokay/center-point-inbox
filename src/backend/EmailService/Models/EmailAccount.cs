namespace EmailService.Models;

public class EmailAccount
{
    public Guid Id { get; set; }
    public Guid UserId { get; set; }
    public Guid TenantId { get; set; }
    public required string Email { get; set; }
    public required string DisplayName { get; set; }
    public required string Provider { get; set; } // "imap", "gmail-imap", "outlook-imap", etc.
    public required string ImapHost { get; set; }
    public int ImapPort { get; set; } = 993;
    public required string SmtpHost { get; set; }
    public int SmtpPort { get; set; } = 587;
    public required string Encryption { get; set; } // "ssl", "tls", "none"
    public required string PasswordEncrypted { get; set; }
    public bool IsActive { get; set; } = true;
    public string? LastError { get; set; }
    public DateTime? LastSyncAt { get; set; }
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
    public DateTime UpdatedAt { get; set; } = DateTime.UtcNow;
}
