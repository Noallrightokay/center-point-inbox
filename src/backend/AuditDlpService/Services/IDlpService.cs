using CenterPointInbox.AuditDlpService.Models;

namespace CenterPointInbox.AuditDlpService.Services;

/// <summary>
/// Service interface for DLP violation management and dashboard data.
/// </summary>
public interface IDlpService
{
    /// <summary>
    /// Builds the DLP dashboard response with violation statistics.
    /// </summary>
    /// <param name="tenantId">The tenant to scope the query to.</param>
    /// <returns>Dashboard data including violation counts by severity and recent violations.</returns>
    Task<DlpDashboardResponse> GetDlpDashboardAsync(Guid tenantId);

    /// <summary>
    /// Retrieves DLP violations for a tenant with optional filtering.
    /// </summary>
    /// <param name="tenantId">The tenant to scope the query to.</param>
    /// <param name="resolved">Optional filter by resolved status.</param>
    /// <param name="severity">Optional filter by severity level.</param>
    /// <returns>A list of DLP violation DTOs.</returns>
    Task<List<DlpViolationDto>> GetViolationsAsync(Guid tenantId, bool? resolved, string? severity);

    /// <summary>
    /// Marks a DLP violation as resolved by an admin user.
    /// </summary>
    /// <param name="tenantId">The tenant to scope the operation to.</param>
    /// <param name="violationId">The violation to resolve.</param>
    /// <param name="resolvedByUserId">The admin user resolving the violation.</param>
    Task ResolveViolationAsync(Guid tenantId, Guid violationId, Guid resolvedByUserId);

    /// <summary>
    /// Removes quarantine status from a file, allowing downloads again.
    /// </summary>
    /// <param name="tenantId">The tenant to scope the operation to.</param>
    /// <param name="fileId">The file to unquarantine.</param>
    /// <param name="userId">The admin user performing the action.</param>
    Task UnquarantineFileAsync(Guid tenantId, Guid fileId, Guid userId);
}
