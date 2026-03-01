using CenterPointInbox.AuditDlpService.Models;

namespace CenterPointInbox.AuditDlpService.Services;

/// <summary>
/// Service interface for querying audit logs and building admin dashboard data.
/// </summary>
public interface IAuditService
{
    /// <summary>
    /// Retrieves a paginated, filtered list of audit log entries for a tenant.
    /// </summary>
    /// <param name="tenantId">The tenant to scope the query to.</param>
    /// <param name="query">Filtering and pagination parameters.</param>
    /// <returns>A paginated list of audit log DTOs with total count.</returns>
    Task<AuditLogListResponse> GetAuditLogsAsync(Guid tenantId, AuditQueryParams query);

    /// <summary>
    /// Retrieves a single audit log entry by ID, scoped to a tenant.
    /// </summary>
    /// <param name="tenantId">The tenant to scope the query to.</param>
    /// <param name="logId">The unique identifier of the audit log entry.</param>
    /// <returns>The audit log DTO.</returns>
    Task<AuditLogDto> GetAuditLogAsync(Guid tenantId, Guid logId);

    /// <summary>
    /// Builds the admin dashboard response with aggregated platform metrics.
    /// </summary>
    /// <param name="tenantId">The tenant to scope the query to.</param>
    /// <returns>Dashboard data including user counts, file counts, recent activity, and violation summary.</returns>
    Task<AdminDashboardResponse> GetAdminDashboardAsync(Guid tenantId);
}
