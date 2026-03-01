using CenterPointInbox.MergerService.Models;
using CenterPointInbox.MergerService.Services;
using CenterPointInbox.Shared.Middleware;
using CenterPointInbox.Shared.Models;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

namespace CenterPointInbox.MergerService.Controllers;

/// <summary>
/// API controller for the unified file feed. Aggregates files from all connected
/// cloud providers (Google Drive, OneDrive/SharePoint) into a single view with
/// filtering, sorting, pagination, and download support.
/// </summary>
[ApiController]
[Route("api/files")]
[Authorize]
public class FilesController : ControllerBase
{
    private readonly IMergerService _mergerService;
    private readonly ILogger<FilesController> _logger;

    public FilesController(IMergerService mergerService, ILogger<FilesController> logger)
    {
        _mergerService = mergerService;
        _logger = logger;
    }

    /// <summary>
    /// Returns a paginated, unified file feed across all connected providers.
    /// Supports filtering by provider and file name search, sorting by name/modified/size.
    /// </summary>
    [HttpGet]
    [ProducesResponseType(typeof(ApiResponse<UnifiedFileFeedResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    public async Task<IActionResult> GetUnifiedFeed([FromQuery] FileFeedQuery query)
    {
        var userContext = UserContext.FromClaimsPrincipal(User);
        var correlationId = HttpContext.GetCorrelationId();

        _logger.LogInformation(
            "Unified feed requested by user {UserId} (page={Page}, provider={Provider}, search={Search}) [CorrelationId: {CorrelationId}]",
            userContext.UserId, query.Page, query.Provider ?? "all", query.Search ?? "none", correlationId);

        var result = await _mergerService.GetUnifiedFeedAsync(
            userContext.UserId, userContext.TenantId, query);

        var pagination = PaginationMeta.Create(result.Page, result.PageSize, result.TotalCount);

        return Ok(ApiResponse<UnifiedFileFeedResponse>.Success(result, pagination, correlationId));
    }

    /// <summary>
    /// Returns metadata for a single file by its platform ID.
    /// </summary>
    [HttpGet("{fileId:guid}")]
    [ProducesResponseType(typeof(ApiResponse<UnifiedFileDto>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> GetFile([FromRoute] Guid fileId)
    {
        var userContext = UserContext.FromClaimsPrincipal(User);
        var correlationId = HttpContext.GetCorrelationId();

        _logger.LogInformation(
            "File metadata requested for {FileId} by user {UserId} [CorrelationId: {CorrelationId}]",
            fileId, userContext.UserId, correlationId);

        var result = await _mergerService.GetFileAsync(
            userContext.UserId, userContext.TenantId, fileId);

        return Ok(ApiResponse<UnifiedFileDto>.Success(result, correlationId));
    }

    /// <summary>
    /// Downloads the binary content of a file. Delegates to the appropriate provider
    /// service (Google or Microsoft). Quarantined files will return a 400 error.
    /// </summary>
    [HttpGet("{fileId:guid}/download")]
    [ProducesResponseType(typeof(FileContentResult), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status400BadRequest)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> DownloadFile([FromRoute] Guid fileId)
    {
        var userContext = UserContext.FromClaimsPrincipal(User);
        var correlationId = HttpContext.GetCorrelationId();

        _logger.LogInformation(
            "File download requested for {FileId} by user {UserId} [CorrelationId: {CorrelationId}]",
            fileId, userContext.UserId, correlationId);

        var content = await _mergerService.DownloadFileAsync(
            userContext.UserId, userContext.TenantId, fileId);

        // Retrieve file metadata for content-type and filename headers
        var fileMeta = await _mergerService.GetFileAsync(
            userContext.UserId, userContext.TenantId, fileId);

        return File(content, fileMeta.MimeType, fileMeta.Name);
    }
}
