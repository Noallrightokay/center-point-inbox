using EmailService.Models;

namespace EmailService.Services;

public interface ISmtpService
{
    Task SendAsync(EmailAccount account, string decryptedPassword, SendEmailRequest request, CancellationToken ct = default);
}
