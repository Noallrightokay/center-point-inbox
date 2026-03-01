namespace EmailService.Configuration;

public class EmailSettings
{
    public int MaxSyncEmails { get; set; } = 200;
    public int SyncIntervalMinutes { get; set; } = 5;
    public int ConnectionTimeoutSeconds { get; set; } = 30;
    public int MaxAttachmentSizeMb { get; set; } = 25;
}

public class ImapConnectionConfig
{
    public required string Email { get; set; }
    public required string PasswordEncrypted { get; set; }
    public required string ImapHost { get; set; }
    public int ImapPort { get; set; } = 993;
    public required string SmtpHost { get; set; }
    public int SmtpPort { get; set; } = 587;
    public string Encryption { get; set; } = "tls";
}
