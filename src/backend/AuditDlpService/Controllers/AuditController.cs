using System.Security.Claims;
using CenterPointInbox.AuditDlpService.Models;
using CenterPointInbox.AuditDlpService.Services;
using CenterPointInbox.Shared.Models;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

namespace CenterPointInbox.AuditDlpService.Controllers;

/// <summary>
/// API controller for querying audit logs.
/// Restricted to admin users only.
/// </summary>
[ApiController]
[Route("api/audit")]
[Authorize(Roles = "Admin")]
public class AuditController : ControllerBase
{
    private readonly IAuditService _auditService;
    private readonly ILogger<AuditController> _logger;

    public AuditController(IAuditService auditService, ILogger<AuditController> logger)
    {
        _auditService = auditService;
        _logger = logger;
    }

    /// <summary>
    /// Retrieves a paginated list of audit log entries for the authenticated tenant.
    /// Supports filtering by user, action, resource type, and date range.
    /// </summary>
    [HttpGet]
    [ProducesResponseType(typeof(ApiResponse<AuditLogListResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status403Forbidden)]
    public async Task<IActionResult> GetAuditLogs([FromQuery] AuditQueryParams query)
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userContext = UserContext.FromClaimsPrincipal(User);

        _logger.LogInformation(
            "Admin {UserId} requesting audit logs for tenant {TenantId} [CorrelationId: {CorrelationId}]",
            userContext.UserId, userContext.TenantId, correlationId);

        var result = await _auditService.GetAuditLogsAsync(userContext.TenantId, query);

        return Ok(ApiResponse<AuditLogListResponse>.Success(result, correlationId));
    }

    /// <summary>
    /// Retrieves a single audit log entry by its unique identifier.
    /// </summary>
    [HttpGet("{id:guid}")]
    [ProducesResponseType(typeof(ApiResponse<AuditLogDto>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status403Forbidden)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> GetAuditLog(Guid id)
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userContext = UserContext.FromClaimsPrincipal(User);

        _logger.LogInformation(
            "Admin {UserId} requesting audit log {LogId} for tenant {TenantId} [CorrelationId: {CorrelationId}]",
            userContext.UserId, id, userContext.TenantId, correlationId);

        var result = await _auditService.GetAuditLogAsync(userContext.TenantId, id);

        return Ok(ApiResponse<AuditLogDto>.Success(result, correlationId));
    }
}
