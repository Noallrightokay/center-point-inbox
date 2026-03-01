using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using EmailService.Models;
using EmailService.Services;
using Shared.Security;
using Shared.Models;

namespace EmailService.Controllers;

[ApiController]
[Route("api/email")]
[Authorize]
public class EmailController : ControllerBase
{
    private readonly EmailDbContext _db;
    private readonly IImapService _imap;
    private readonly ISmtpService _smtp;
    private readonly ILogger<EmailController> _logger;

    public EmailController(EmailDbContext db, IImapService imap, ISmtpService smtp, ILogger<EmailController> logger)
    {
        _db = db;
        _imap = imap;
        _smtp = smtp;
        _logger = logger;
    }

    private UserContext GetUser()
    {
        var userId = User.FindFirst("sub")?.Value ?? throw new UnauthorizedAccessException();
        var tenantId = User.FindFirst("tenantId")?.Value ?? throw new UnauthorizedAccessException();
        var email = User.FindFirst("email")?.Value ?? "";
        return new UserContext
        {
            UserId = Guid.Parse(userId),
            TenantId = Guid.Parse(tenantId),
            Email = email,
        };
    }

    /// <summary>Connect a new IMAP email account</summary>
    [HttpPost("connect")]
    public async Task<IActionResult> ConnectAccount([FromBody] ConnectEmailRequest request, CancellationToken ct)
    {
        var user = GetUser();

        // Auto-detect provider
        var domain = request.Email.Split('@').LastOrDefault()?.ToLower() ?? "";
        var provider = domain switch
        {
            "gmail.com" => "gmail-imap",
            "outlook.com" or "hotmail.com" or "live.com" => "outlook-imap",
            "yahoo.com" => "yahoo-imap",
            "icloud.com" or "me.com" => "icloud-imap",
            _ => "imap",
        };

        var imapHost = request.ImapHost ?? GetDefaultImapHost(domain);
        var imapPort = request.ImapPort ?? 993;
        var smtpHost = request.SmtpHost ?? GetDefaultSmtpHost(domain);
        var smtpPort = request.SmtpPort ?? 587;
        var encryption = request.Encryption ?? "tls";

        // Test connection
        var connected = await _imap.TestConnectionAsync(imapHost, imapPort, request.Email, request.Password, encryption, ct);
        if (!connected)
            return BadRequest(new { message = "Unable to connect. Please check your credentials and server settings." });

        // Encrypt password
        var encryptedPassword = TokenEncryption.Encrypt(request.Password);

        // Upsert account
        var existing = await _db.EmailAccounts
            .FirstOrDefaultAsync(a => a.TenantId == user.TenantId && a.Email == request.Email, ct);

        if (existing != null)
        {
            existing.PasswordEncrypted = encryptedPassword;
            existing.ImapHost = imapHost;
            existing.ImapPort = imapPort;
            existing.SmtpHost = smtpHost;
            existing.SmtpPort = smtpPort;
            existing.Encryption = encryption;
            existing.IsActive = true;
            existing.LastError = null;
            existing.UpdatedAt = DateTime.UtcNow;
        }
        else
        {
            _db.EmailAccounts.Add(new EmailAccount
            {
                UserId = user.UserId,
                TenantId = user.TenantId,
                Email = request.Email,
                DisplayName = request.Email.Split('@')[0],
                Provider = provider,
                ImapHost = imapHost,
                ImapPort = imapPort,
                SmtpHost = smtpHost,
                SmtpPort = smtpPort,
                Encryption = encryption,
                PasswordEncrypted = encryptedPassword,
            });
        }

        await _db.SaveChangesAsync(ct);
        return Ok(new { message = "Account connected successfully" });
    }

    /// <summary>List connected email accounts</summary>
    [HttpGet("accounts")]
    public async Task<IActionResult> ListAccounts(CancellationToken ct)
    {
        var user = GetUser();
        var accounts = await _db.EmailAccounts
            .Where(a => a.TenantId == user.TenantId && a.UserId == user.UserId)
            .OrderBy(a => a.CreatedAt)
            .Select(a => new EmailAccountResponse(a.Id, a.Email, a.DisplayName, a.Provider, a.IsActive, a.LastError, a.LastSyncAt))
            .ToListAsync(ct);
        return Ok(accounts);
    }

    /// <summary>Delete an email account connection</summary>
    [HttpDelete("accounts/{accountId:guid}")]
    public async Task<IActionResult> DeleteAccount(Guid accountId, CancellationToken ct)
    {
        var user = GetUser();
        var account = await _db.EmailAccounts.FirstOrDefaultAsync(a => a.Id == accountId && a.UserId == user.UserId, ct);
        if (account == null) return NotFound();
        _db.EmailAccounts.Remove(account);
        await _db.SaveChangesAsync(ct);
        return NoContent();
    }

    /// <summary>Sync emails from all connected accounts</summary>
    [HttpPost("sync")]
    public async Task<IActionResult> SyncEmails(CancellationToken ct)
    {
        var user = GetUser();
        var accounts = await _db.EmailAccounts
            .Where(a => a.UserId == user.UserId && a.IsActive)
            .ToListAsync(ct);

        var synced = 0;
        foreach (var account in accounts)
        {
            try
            {
                var password = TokenEncryption.Decrypt(account.PasswordEncrypted);
                var emails = await _imap.FetchEmailsAsync(account, password, 50, ct);
                foreach (var email in emails)
                {
                    var exists = await _db.EmailMessages.AnyAsync(
                        m => m.AccountId == account.Id && m.MessageUid == email.MessageUid, ct);
                    if (!exists)
                    {
                        _db.EmailMessages.Add(email);
                        synced++;
                    }
                }
                account.LastSyncAt = DateTime.UtcNow;
                account.LastError = null;
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Sync failed for account {AccountId}", account.Id);
                account.LastError = ex.Message;
            }
        }

        await _db.SaveChangesAsync(ct);
        return Ok(new { synced });
    }

    /// <summary>Get emails for the inbox view</summary>
    [HttpGet("messages")]
    public async Task<IActionResult> GetMessages(
        [FromQuery] string? search, [FromQuery] string? accountId,
        [FromQuery] int limit = 50, [FromQuery] string? cursor, CancellationToken ct = default)
    {
        var user = GetUser();
        var query = _db.EmailMessages
            .Where(m => m.TenantId == user.TenantId && m.UserId == user.UserId);

        if (!string.IsNullOrWhiteSpace(accountId) && Guid.TryParse(accountId, out var aid))
            query = query.Where(m => m.AccountId == aid);

        if (!string.IsNullOrWhiteSpace(search))
            query = query.Where(m => m.Subject.Contains(search) || (m.FromAddress != null && m.FromAddress.Contains(search)));

        if (!string.IsNullOrWhiteSpace(cursor) && DateTime.TryParse(cursor, out var cursorDate))
            query = query.Where(m => m.ReceivedAt < cursorDate);

        var total = await query.CountAsync(ct);
        var items = await query
            .OrderByDescending(m => m.ReceivedAt)
            .Take(limit)
            .Select(m => new EmailMessageResponse(
                m.Id, m.Subject, m.Snippet, m.FromAddress, m.FromName,
                m.ToAddresses, m.IsRead, m.IsStarred, m.HasAttachments, m.ReceivedAt))
            .ToListAsync(ct);

        var nextCursor = items.Count == limit ? items.Last().ReceivedAt.ToString("O") : null;
        return Ok(new EmailFeedResponse(items.ToArray(), total, nextCursor, nextCursor != null));
    }

    /// <summary>Send an email via SMTP</summary>
    [HttpPost("send")]
    public async Task<IActionResult> SendEmail([FromQuery] Guid accountId, [FromBody] SendEmailRequest request, CancellationToken ct)
    {
        var user = GetUser();
        var account = await _db.EmailAccounts.FirstOrDefaultAsync(a => a.Id == accountId && a.UserId == user.UserId, ct);
        if (account == null) return NotFound(new { message = "Account not found" });

        var password = TokenEncryption.Decrypt(account.PasswordEncrypted);
        await _smtp.SendAsync(account, password, request, ct);
        return Ok(new { message = "Email sent" });
    }

    private static string GetDefaultImapHost(string domain) => domain switch
    {
        "gmail.com" => "imap.gmail.com",
        "outlook.com" or "hotmail.com" or "live.com" => "outlook.office365.com",
        "yahoo.com" => "imap.mail.yahoo.com",
        "icloud.com" or "me.com" => "imap.mail.me.com",
        "aol.com" => "imap.aol.com",
        "zoho.com" => "imap.zoho.com",
        _ => $"imap.{domain}",
    };

    private static string GetDefaultSmtpHost(string domain) => domain switch
    {
        "gmail.com" => "smtp.gmail.com",
        "outlook.com" or "hotmail.com" or "live.com" => "smtp.office365.com",
        "yahoo.com" => "smtp.mail.yahoo.com",
        "icloud.com" or "me.com" => "smtp.mail.me.com",
        "aol.com" => "smtp.aol.com",
        "zoho.com" => "smtp.zoho.com",
        _ => $"smtp.{domain}",
    };
}
