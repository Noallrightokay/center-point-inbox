using MailKit.Net.Smtp;
using MailKit.Security;
using MimeKit;
using EmailService.Models;

namespace EmailService.Services;

public class SmtpService : ISmtpService
{
    private readonly ILogger<SmtpService> _logger;

    public SmtpService(ILogger<SmtpService> logger)
    {
        _logger = logger;
    }

    public async Task SendAsync(EmailAccount account, string decryptedPassword, SendEmailRequest request, CancellationToken ct = default)
    {
        var message = new MimeMessage();
        message.From.Add(new MailboxAddress(account.DisplayName, account.Email));
        message.To.AddRange(request.To.Split(',', StringSplitOptions.TrimEntries).Select(e => MailboxAddress.Parse(e)));
        if (!string.IsNullOrWhiteSpace(request.Cc))
            message.Cc.AddRange(request.Cc.Split(',', StringSplitOptions.TrimEntries).Select(e => MailboxAddress.Parse(e)));
        if (!string.IsNullOrWhiteSpace(request.Bcc))
            message.Bcc.AddRange(request.Bcc.Split(',', StringSplitOptions.TrimEntries).Select(e => MailboxAddress.Parse(e)));

        message.Subject = request.Subject;

        var bodyBuilder = new BodyBuilder();
        if (request.IsHtml)
            bodyBuilder.HtmlBody = request.Body;
        else
            bodyBuilder.TextBody = request.Body;
        message.Body = bodyBuilder.ToMessageBody();

        using var client = new SmtpClient();
        var secureOption = account.Encryption switch
        {
            "ssl" => SecureSocketOptions.SslOnConnect,
            "tls" => SecureSocketOptions.StartTls,
            _ => SecureSocketOptions.None,
        };

        await client.ConnectAsync(account.SmtpHost, account.SmtpPort, secureOption, ct);
        await client.AuthenticateAsync(account.Email, decryptedPassword, ct);
        await client.SendAsync(message, ct);
        await client.DisconnectAsync(true, ct);

        _logger.LogInformation("Email sent from {From} to {To}", account.Email, request.To);
    }
}
