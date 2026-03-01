using CenterPointInbox.AuditDlpService.Models;
using CenterPointInbox.Shared.Contracts;
using MassTransit;
using Microsoft.EntityFrameworkCore;

namespace CenterPointInbox.AuditDlpService.Services;

/// <summary>
/// Implementation of <see cref="IDlpService"/> that manages DLP violations,
/// quarantine operations, and builds DLP dashboard data.
/// </summary>
public class DlpService : IDlpService
{
    private readonly AuditDlpDbContext _dbContext;
    private readonly IPublishEndpoint _publishEndpoint;
    private readonly ILogger<DlpService> _logger;

    public DlpService(
        AuditDlpDbContext dbContext,
        IPublishEndpoint publishEndpoint,
        ILogger<DlpService> logger)
    {
        _dbContext = dbContext;
        _publishEndpoint = publishEndpoint;
        _logger = logger;
    }

    /// <inheritdoc />
    public async Task<DlpDashboardResponse> GetDlpDashboardAsync(Guid tenantId)
    {
        _logger.LogDebug("Building DLP dashboard for tenant {TenantId}", tenantId);

        var violationsQuery = _dbContext.DlpViolations
            .AsNoTracking()
            .Where(v => v.TenantId == tenantId);

        var totalViolations = await violationsQuery.LongCountAsync();
        var unresolvedCount = await violationsQuery.Where(v => !v.Resolved).LongCountAsync();

        // Group violations by severity
        var bySeverity = await violationsQuery
            .GroupBy(v => v.Severity)
            .Select(g => new { Severity = g.Key, Count = g.LongCount() })
            .ToListAsync();

        var bySeverityDict = bySeverity.ToDictionary(
            x => x.Severity.ToString(),
            x => x.Count);

        // Ensure all severity levels are present in the dictionary
        foreach (var severity in Enum.GetValues<Severity>())
        {
            bySeverityDict.TryAdd(severity.ToString(), 0);
        }

        // Recent violations: last 10 entries
        var recentViolations = await violationsQuery
            .OrderByDescending(v => v.CreatedAt)
            .Take(10)
            .Select(v => new DlpViolationDto
            {
                Id = v.Id,
                FileId = v.FileId,
                ViolationType = v.ViolationType,
                Severity = v.Severity.ToString(),
                Description = v.Description,
                PatternMatched = v.PatternMatched,
                Resolved = v.Resolved,
                CreatedAt = v.CreatedAt
            })
            .ToListAsync();

        _logger.LogDebug(
            "DLP dashboard for tenant {TenantId}: Total={Total}, Unresolved={Unresolved}",
            tenantId, totalViolations, unresolvedCount);

        return new DlpDashboardResponse
        {
            TotalViolations = totalViolations,
            UnresolvedCount = unresolvedCount,
            BySeverity = bySeverityDict,
            RecentViolations = recentViolations
        };
    }

    /// <inheritdoc />
    public async Task<List<DlpViolationDto>> GetViolationsAsync(Guid tenantId, bool? resolved, string? severity)
    {
        _logger.LogDebug(
            "Querying DLP violations for tenant {TenantId}: Resolved={Resolved}, Severity={Severity}",
            tenantId, resolved, severity);

        var query = _dbContext.DlpViolations
            .AsNoTracking()
            .Where(v => v.TenantId == tenantId);

        if (resolved.HasValue)
        {
            query = query.Where(v => v.Resolved == resolved.Value);
        }

        if (!string.IsNullOrWhiteSpace(severity) && Enum.TryParse<Severity>(severity, ignoreCase: true, out var severityEnum))
        {
            query = query.Where(v => v.Severity == severityEnum);
        }

        var violations = await query
            .OrderByDescending(v => v.CreatedAt)
            .Take(200)
            .Select(v => new DlpViolationDto
            {
                Id = v.Id,
                FileId = v.FileId,
                ViolationType = v.ViolationType,
                Severity = v.Severity.ToString(),
                Description = v.Description,
                PatternMatched = v.PatternMatched,
                Resolved = v.Resolved,
                CreatedAt = v.CreatedAt
            })
            .ToListAsync();

        _logger.LogDebug("Retrieved {Count} DLP violations for tenant {TenantId}", violations.Count, tenantId);

        return violations;
    }

    /// <inheritdoc />
    public async Task ResolveViolationAsync(Guid tenantId, Guid violationId, Guid resolvedByUserId)
    {
        _logger.LogInformation(
            "Resolving DLP violation {ViolationId} for tenant {TenantId} by user {UserId}",
            violationId, tenantId, resolvedByUserId);

        var violation = await _dbContext.DlpViolations
            .Where(v => v.TenantId == tenantId && v.Id == violationId)
            .FirstOrDefaultAsync();

        if (violation is null)
        {
            throw new KeyNotFoundException($"DLP violation with ID '{violationId}' was not found.");
        }

        if (violation.Resolved)
        {
            _logger.LogWarning("DLP violation {ViolationId} is already resolved", violationId);
            return;
        }

        violation.Resolved = true;
        violation.ResolvedByUserId = resolvedByUserId;
        violation.ResolvedAt = DateTimeOffset.UtcNow;

        await _dbContext.SaveChangesAsync();

        // Publish audit event for the resolution
        await _publishEndpoint.Publish(new AuditEvent
        {
            TenantId = tenantId,
            UserId = resolvedByUserId,
            Action = "dlp.violation.resolved",
            ResourceType = "dlp_violation",
            ResourceId = violationId.ToString(),
            Metadata = new Dictionary<string, string>
            {
                ["violationType"] = violation.ViolationType,
                ["severity"] = violation.Severity.ToString(),
                ["fileId"] = violation.FileId.ToString()
            },
            OccurredAt = DateTimeOffset.UtcNow
        });

        _logger.LogInformation(
            "DLP violation {ViolationId} resolved successfully by user {UserId}",
            violationId, resolvedByUserId);
    }

    /// <inheritdoc />
    public async Task UnquarantineFileAsync(Guid tenantId, Guid fileId, Guid userId)
    {
        _logger.LogInformation(
            "Unquarantining file {FileId} for tenant {TenantId} by user {UserId}",
            fileId, tenantId, userId);

        var file = await _dbContext.FilesMetadata
            .Where(f => f.TenantId == tenantId && f.Id == fileId)
            .FirstOrDefaultAsync();

        if (file is null)
        {
            throw new KeyNotFoundException($"File with ID '{fileId}' was not found.");
        }

        if (!file.IsQuarantined)
        {
            _logger.LogWarning("File {FileId} is not currently quarantined", fileId);
            return;
        }

        // Capture reason before clearing for the audit event
        var previousReason = file.QuarantineReason ?? "N/A";

        file.IsQuarantined = false;
        file.QuarantineReason = null;
        file.UpdatedAt = DateTimeOffset.UtcNow;

        await _dbContext.SaveChangesAsync();

        // Publish audit event for the unquarantine action
        await _publishEndpoint.Publish(new AuditEvent
        {
            TenantId = tenantId,
            UserId = userId,
            Action = "dlp.file.unquarantined",
            ResourceType = "file",
            ResourceId = fileId.ToString(),
            Metadata = new Dictionary<string, string>
            {
                ["fileName"] = file.FileName,
                ["previousReason"] = previousReason
            },
            OccurredAt = DateTimeOffset.UtcNow
        });

        _logger.LogInformation(
            "File {FileId} unquarantined successfully by user {UserId}",
            fileId, userId);
    }
}
