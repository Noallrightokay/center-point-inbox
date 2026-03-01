namespace EmailService.Models;

public record ConnectEmailRequest(
    string Email,
    string Password,
    string? ImapHost,
    int? ImapPort,
    string? SmtpHost,
    int? SmtpPort,
    string? Encryption
);

public record EmailAccountResponse(
    Guid Id,
    string Email,
    string DisplayName,
    string Provider,
    bool IsActive,
    string? LastError,
    DateTime? LastSyncAt
);

public record SendEmailRequest(
    string To,
    string? Cc,
    string? Bcc,
    string Subject,
    string Body,
    bool IsHtml = false
);

public record EmailMessageResponse(
    Guid Id,
    string Subject,
    string? Snippet,
    string FromAddress,
    string? FromName,
    string[] ToAddresses,
    bool IsRead,
    bool IsStarred,
    bool HasAttachments,
    DateTime ReceivedAt
);

public record EmailFeedResponse(
    EmailMessageResponse[] Items,
    int TotalCount,
    string? NextCursor,
    bool HasMore
);
