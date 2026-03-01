using MailKit;
using MailKit.Net.Imap;
using MailKit.Search;
using MailKit.Security;
using MimeKit;
using EmailService.Models;

namespace EmailService.Services;

public class ImapService : IImapService
{
    private readonly ILogger<ImapService> _logger;

    public ImapService(ILogger<ImapService> logger)
    {
        _logger = logger;
    }

    public async Task<bool> TestConnectionAsync(string host, int port, string email, string password, string encryption, CancellationToken ct = default)
    {
        using var client = new ImapClient();
        try
        {
            var secureOption = encryption switch
            {
                "ssl" => SecureSocketOptions.SslOnConnect,
                "tls" => SecureSocketOptions.StartTls,
                _ => SecureSocketOptions.None,
            };
            await client.ConnectAsync(host, port, secureOption, ct);
            await client.AuthenticateAsync(email, password, ct);
            await client.DisconnectAsync(true, ct);
            return true;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "IMAP connection test failed for {Email} at {Host}:{Port}", email, host, port);
            return false;
        }
    }

    public async Task<List<EmailMessage>> FetchEmailsAsync(EmailAccount account, string decryptedPassword, int maxCount = 50, CancellationToken ct = default)
    {
        var messages = new List<EmailMessage>();
        using var client = new ImapClient();

        var secureOption = account.Encryption switch
        {
            "ssl" => SecureSocketOptions.SslOnConnect,
            "tls" => SecureSocketOptions.StartTls,
            _ => SecureSocketOptions.None,
        };

        await client.ConnectAsync(account.ImapHost, account.ImapPort, secureOption, ct);
        await client.AuthenticateAsync(account.Email, decryptedPassword, ct);

        var inbox = client.Inbox;
        await inbox.OpenAsync(FolderAccess.ReadOnly, ct);

        var count = Math.Min(inbox.Count, maxCount);
        var start = Math.Max(0, inbox.Count - count);

        for (int i = inbox.Count - 1; i >= start && messages.Count < maxCount; i--)
        {
            var message = await inbox.GetMessageAsync(i, ct);
            var uid = await inbox.GetUidAsync(i, ct);

            messages.Add(new EmailMessage
            {
                AccountId = account.Id,
                TenantId = account.TenantId,
                UserId = account.UserId,
                MessageUid = uid.ToString(),
                Subject = message.Subject ?? "(no subject)",
                Snippet = message.TextBody?.Length > 200 ? message.TextBody[..200] : message.TextBody,
                BodyHtml = message.HtmlBody,
                BodyText = message.TextBody,
                FromAddress = message.From.Mailboxes.FirstOrDefault()?.Address ?? "",
                FromName = message.From.Mailboxes.FirstOrDefault()?.Name,
                ToAddresses = message.To.Mailboxes.Select(m => m.Address).ToArray(),
                CcAddresses = message.Cc.Mailboxes.Select(m => m.Address).ToArray(),
                BccAddresses = message.Bcc.Mailboxes.Select(m => m.Address).ToArray(),
                IsRead = false,
                IsStarred = false,
                HasAttachments = message.Attachments.Any(),
                ThreadId = message.References?.FirstOrDefault()?.ToString(),
                InReplyTo = message.InReplyTo,
                SizeBytes = message.ToString().Length,
                ReceivedAt = message.Date.UtcDateTime,
            });
        }

        await client.DisconnectAsync(true, ct);
        return messages;
    }
}
