using System.Security.Claims;
using CenterPointInbox.AuditDlpService.Models;
using CenterPointInbox.AuditDlpService.Services;
using CenterPointInbox.Shared.Models;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

namespace CenterPointInbox.AuditDlpService.Controllers;

/// <summary>
/// API controller for admin dashboard and DLP management endpoints.
/// Restricted to admin users only.
/// </summary>
[ApiController]
[Route("api/admin")]
[Authorize(Roles = "Admin")]
public class AdminController : ControllerBase
{
    private readonly IAuditService _auditService;
    private readonly IDlpService _dlpService;
    private readonly ILogger<AdminController> _logger;

    public AdminController(
        IAuditService auditService,
        IDlpService dlpService,
        ILogger<AdminController> logger)
    {
        _auditService = auditService;
        _dlpService = dlpService;
        _logger = logger;
    }

    /// <summary>
    /// Returns the admin dashboard with aggregated platform metrics
    /// including user counts, file counts, translation counts,
    /// recent activity, and violation summary.
    /// </summary>
    [HttpGet("dashboard")]
    [ProducesResponseType(typeof(ApiResponse<AdminDashboardResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status403Forbidden)]
    public async Task<IActionResult> GetAdminDashboard()
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userContext = UserContext.FromClaimsPrincipal(User);

        _logger.LogInformation(
            "Admin {UserId} requesting dashboard for tenant {TenantId} [CorrelationId: {CorrelationId}]",
            userContext.UserId, userContext.TenantId, correlationId);

        var result = await _auditService.GetAdminDashboardAsync(userContext.TenantId);

        return Ok(ApiResponse<AdminDashboardResponse>.Success(result, correlationId));
    }

    /// <summary>
    /// Returns the DLP dashboard with violation statistics including
    /// total violations, unresolved count, breakdown by severity, and recent violations.
    /// </summary>
    [HttpGet("dlp")]
    [ProducesResponseType(typeof(ApiResponse<DlpDashboardResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status403Forbidden)]
    public async Task<IActionResult> GetDlpDashboard()
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userContext = UserContext.FromClaimsPrincipal(User);

        _logger.LogInformation(
            "Admin {UserId} requesting DLP dashboard for tenant {TenantId} [CorrelationId: {CorrelationId}]",
            userContext.UserId, userContext.TenantId, correlationId);

        var result = await _dlpService.GetDlpDashboardAsync(userContext.TenantId);

        return Ok(ApiResponse<DlpDashboardResponse>.Success(result, correlationId));
    }

    /// <summary>
    /// Returns a list of DLP violations with optional filters for resolved status and severity.
    /// </summary>
    [HttpGet("dlp/violations")]
    [ProducesResponseType(typeof(ApiResponse<List<DlpViolationDto>>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status403Forbidden)]
    public async Task<IActionResult> GetViolations(
        [FromQuery] bool? resolved,
        [FromQuery] string? severity)
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userContext = UserContext.FromClaimsPrincipal(User);

        _logger.LogInformation(
            "Admin {UserId} requesting DLP violations for tenant {TenantId}: Resolved={Resolved}, Severity={Severity} [CorrelationId: {CorrelationId}]",
            userContext.UserId, userContext.TenantId, resolved, severity, correlationId);

        var result = await _dlpService.GetViolationsAsync(userContext.TenantId, resolved, severity);

        return Ok(ApiResponse<List<DlpViolationDto>>.Success(result, correlationId));
    }

    /// <summary>
    /// Marks a DLP violation as resolved by the authenticated admin user.
    /// </summary>
    [HttpPost("dlp/violations/{id:guid}/resolve")]
    [ProducesResponseType(typeof(ApiResponse), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status403Forbidden)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> ResolveViolation(Guid id)
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userContext = UserContext.FromClaimsPrincipal(User);

        _logger.LogInformation(
            "Admin {UserId} resolving DLP violation {ViolationId} for tenant {TenantId} [CorrelationId: {CorrelationId}]",
            userContext.UserId, id, userContext.TenantId, correlationId);

        await _dlpService.ResolveViolationAsync(userContext.TenantId, id, userContext.UserId);

        return Ok(ApiResponse.Success(correlationId));
    }

    /// <summary>
    /// Removes quarantine status from a file, allowing it to be downloaded again.
    /// </summary>
    [HttpPost("dlp/files/{fileId:guid}/unquarantine")]
    [ProducesResponseType(typeof(ApiResponse), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status403Forbidden)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> UnquarantineFile(Guid fileId)
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userContext = UserContext.FromClaimsPrincipal(User);

        _logger.LogInformation(
            "Admin {UserId} unquarantining file {FileId} for tenant {TenantId} [CorrelationId: {CorrelationId}]",
            userContext.UserId, fileId, userContext.TenantId, correlationId);

        await _dlpService.UnquarantineFileAsync(userContext.TenantId, fileId, userContext.UserId);

        return Ok(ApiResponse.Success(correlationId));
    }
}
