using CenterPointInbox.AuditDlpService.Models;
using Microsoft.EntityFrameworkCore;

namespace CenterPointInbox.AuditDlpService.Services;

/// <summary>
/// Implementation of <see cref="IAuditService"/> that queries audit logs
/// from PostgreSQL and builds admin dashboard aggregations.
/// </summary>
public class AuditService : IAuditService
{
    private readonly AuditDlpDbContext _dbContext;
    private readonly ILogger<AuditService> _logger;

    public AuditService(AuditDlpDbContext dbContext, ILogger<AuditService> logger)
    {
        _dbContext = dbContext;
        _logger = logger;
    }

    /// <inheritdoc />
    public async Task<AuditLogListResponse> GetAuditLogsAsync(Guid tenantId, AuditQueryParams query)
    {
        _logger.LogDebug(
            "Querying audit logs for tenant {TenantId} with filters: UserId={UserId}, Action={Action}, ResourceType={ResourceType}, StartDate={StartDate}, EndDate={EndDate}, Page={Page}, PageSize={PageSize}",
            tenantId, query.UserId, query.Action, query.ResourceType, query.StartDate, query.EndDate, query.Page, query.PageSize);

        var baseQuery = _dbContext.AuditLogs
            .AsNoTracking()
            .Where(a => a.TenantId == tenantId);

        // Apply optional filters
        if (query.UserId.HasValue)
        {
            baseQuery = baseQuery.Where(a => a.UserId == query.UserId.Value);
        }

        if (!string.IsNullOrWhiteSpace(query.Action))
        {
            baseQuery = baseQuery.Where(a => a.Action == query.Action);
        }

        if (!string.IsNullOrWhiteSpace(query.ResourceType))
        {
            baseQuery = baseQuery.Where(a => a.ResourceType == query.ResourceType);
        }

        if (query.StartDate.HasValue)
        {
            baseQuery = baseQuery.Where(a => a.CreatedAt >= query.StartDate.Value);
        }

        if (query.EndDate.HasValue)
        {
            baseQuery = baseQuery.Where(a => a.CreatedAt <= query.EndDate.Value);
        }

        // Get total count for pagination
        var totalCount = await baseQuery.LongCountAsync();

        // Ensure valid pagination values
        var page = Math.Max(1, query.Page);
        var pageSize = Math.Clamp(query.PageSize, 1, 100);

        // Fetch page of results sorted by most recent first
        var logs = await baseQuery
            .OrderByDescending(a => a.CreatedAt)
            .Skip((page - 1) * pageSize)
            .Take(pageSize)
            .Select(a => new AuditLogDto
            {
                Id = a.Id,
                UserId = a.UserId,
                Action = a.Action,
                ResourceType = a.ResourceType,
                ResourceId = a.ResourceId,
                IpAddress = a.IpAddress,
                Metadata = a.Metadata,
                CreatedAt = a.CreatedAt
            })
            .ToListAsync();

        _logger.LogDebug(
            "Retrieved {Count} audit logs out of {Total} for tenant {TenantId}",
            logs.Count, totalCount, tenantId);

        return new AuditLogListResponse
        {
            Logs = logs,
            TotalCount = totalCount,
            Page = page,
            PageSize = pageSize
        };
    }

    /// <inheritdoc />
    public async Task<AuditLogDto> GetAuditLogAsync(Guid tenantId, Guid logId)
    {
        _logger.LogDebug("Fetching audit log {LogId} for tenant {TenantId}", logId, tenantId);

        var log = await _dbContext.AuditLogs
            .AsNoTracking()
            .Where(a => a.TenantId == tenantId && a.Id == logId)
            .Select(a => new AuditLogDto
            {
                Id = a.Id,
                UserId = a.UserId,
                Action = a.Action,
                ResourceType = a.ResourceType,
                ResourceId = a.ResourceId,
                IpAddress = a.IpAddress,
                Metadata = a.Metadata,
                CreatedAt = a.CreatedAt
            })
            .FirstOrDefaultAsync();

        if (log is null)
        {
            throw new KeyNotFoundException($"Audit log with ID '{logId}' was not found.");
        }

        return log;
    }

    /// <inheritdoc />
    public async Task<AdminDashboardResponse> GetAdminDashboardAsync(Guid tenantId)
    {
        _logger.LogDebug("Building admin dashboard for tenant {TenantId}", tenantId);

        // Count distinct users from audit logs as a proxy for active users
        var totalUsers = await _dbContext.AuditLogs
            .AsNoTracking()
            .Where(a => a.TenantId == tenantId)
            .Select(a => a.UserId)
            .Distinct()
            .LongCountAsync();

        // Count total files tracked
        var totalFiles = await _dbContext.FilesMetadata
            .AsNoTracking()
            .Where(f => f.TenantId == tenantId)
            .LongCountAsync();

        // Count translation-related audit events
        var totalTranslations = await _dbContext.AuditLogs
            .AsNoTracking()
            .Where(a => a.TenantId == tenantId && a.Action.Contains("translation"))
            .LongCountAsync();

        // Recent activity: last 10 audit log entries
        var recentActivity = await _dbContext.AuditLogs
            .AsNoTracking()
            .Where(a => a.TenantId == tenantId)
            .OrderByDescending(a => a.CreatedAt)
            .Take(10)
            .Select(a => new AuditLogDto
            {
                Id = a.Id,
                UserId = a.UserId,
                Action = a.Action,
                ResourceType = a.ResourceType,
                ResourceId = a.ResourceId,
                IpAddress = a.IpAddress,
                Metadata = a.Metadata,
                CreatedAt = a.CreatedAt
            })
            .ToListAsync();

        // Violation summary
        var violationsQuery = _dbContext.DlpViolations
            .AsNoTracking()
            .Where(v => v.TenantId == tenantId);

        var totalViolations = await violationsQuery.LongCountAsync();
        var unresolvedViolations = await violationsQuery.Where(v => !v.Resolved).LongCountAsync();
        var criticalViolations = await violationsQuery.Where(v => v.Severity == Severity.Critical).LongCountAsync();
        var highViolations = await violationsQuery.Where(v => v.Severity == Severity.High).LongCountAsync();

        _logger.LogDebug(
            "Admin dashboard for tenant {TenantId}: Users={Users}, Files={Files}, Translations={Translations}, Violations={Violations}",
            tenantId, totalUsers, totalFiles, totalTranslations, totalViolations);

        return new AdminDashboardResponse
        {
            TotalUsers = totalUsers,
            TotalFiles = totalFiles,
            TotalTranslations = totalTranslations,
            RecentActivity = recentActivity,
            ViolationSummary = new ViolationSummary
            {
                Total = totalViolations,
                Unresolved = unresolvedViolations,
                Critical = criticalViolations,
                High = highViolations
            }
        };
    }
}
