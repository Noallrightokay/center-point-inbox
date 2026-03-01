using CenterPointInbox.AuditDlpService.Models;
using CenterPointInbox.Shared.Contracts;
using MassTransit;

namespace CenterPointInbox.AuditDlpService.Services;

/// <summary>
/// MassTransit consumer that persists <see cref="AuditEvent"/> messages
/// to the audit_logs database table.
/// </summary>
public class AuditEventConsumer : IConsumer<AuditEvent>
{
    private readonly AuditDlpDbContext _dbContext;
    private readonly ILogger<AuditEventConsumer> _logger;

    public AuditEventConsumer(AuditDlpDbContext dbContext, ILogger<AuditEventConsumer> logger)
    {
        _dbContext = dbContext;
        _logger = logger;
    }

    public async Task Consume(ConsumeContext<AuditEvent> context)
    {
        var auditEvent = context.Message;

        _logger.LogInformation(
            "Consuming AuditEvent: TenantId={TenantId}, UserId={UserId}, Action={Action}, ResourceType={ResourceType}, ResourceId={ResourceId}",
            auditEvent.TenantId,
            auditEvent.UserId,
            auditEvent.Action,
            auditEvent.ResourceType,
            auditEvent.ResourceId);

        var auditLog = new AuditLog
        {
            Id = Guid.NewGuid(),
            TenantId = auditEvent.TenantId,
            UserId = auditEvent.UserId,
            Action = auditEvent.Action,
            ResourceType = auditEvent.ResourceType,
            ResourceId = auditEvent.ResourceId,
            IpAddress = auditEvent.IpAddress,
            Metadata = auditEvent.Metadata?.ToDictionary(
                kvp => kvp.Key,
                kvp => (object)kvp.Value) ?? new Dictionary<string, object>(),
            CreatedAt = auditEvent.OccurredAt
        };

        _dbContext.AuditLogs.Add(auditLog);
        await _dbContext.SaveChangesAsync();

        _logger.LogDebug(
            "Persisted audit log {AuditLogId} for action {Action} on {ResourceType}/{ResourceId}",
            auditLog.Id, auditLog.Action, auditLog.ResourceType, auditLog.ResourceId);
    }
}
