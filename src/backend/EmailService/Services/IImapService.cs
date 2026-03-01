using EmailService.Models;

namespace EmailService.Services;

public interface IImapService
{
    Task<bool> TestConnectionAsync(string host, int port, string email, string password, string encryption, CancellationToken ct = default);
    Task<List<EmailMessage>> FetchEmailsAsync(EmailAccount account, string decryptedPassword, int maxCount = 50, CancellationToken ct = default);
}
