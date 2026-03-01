using System.Security.Claims;
using CenterPointInbox.MicrosoftProviderService.Models;
using CenterPointInbox.MicrosoftProviderService.Services;
using CenterPointInbox.Shared.Models;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

namespace CenterPointInbox.MicrosoftProviderService.Controllers;

/// <summary>
/// API controller for Microsoft OneDrive/SharePoint file operations.
/// All endpoints require JWT authentication.
/// </summary>
[ApiController]
[Route("api/files/microsoft")]
[Authorize]
public class MicrosoftFilesController : ControllerBase
{
    private readonly IMicrosoftGraphService _microsoftGraphService;
    private readonly ILogger<MicrosoftFilesController> _logger;

    public MicrosoftFilesController(
        IMicrosoftGraphService microsoftGraphService,
        ILogger<MicrosoftFilesController> logger)
    {
        _microsoftGraphService = microsoftGraphService;
        _logger = logger;
    }

    /// <summary>
    /// Lists files from the authenticated user's OneDrive with optional pagination and search.
    /// </summary>
    /// <param name="skipToken">Optional skip token for pagination continuation.</param>
    /// <param name="pageSize">Number of files per page (default 100, max 1000).</param>
    /// <param name="search">Optional search query to filter files by name.</param>
    /// <returns>A paginated list of Microsoft files.</returns>
    [HttpGet]
    [ProducesResponseType(typeof(ApiResponse<MicrosoftFileListResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> ListFiles(
        [FromQuery] string? skipToken = null,
        [FromQuery] int pageSize = 100,
        [FromQuery] string? search = null)
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userId = GetCurrentUserId();

        _logger.LogInformation(
            "Listing Microsoft files for user {UserId}, pageSize={PageSize}, search={Search} [CorrelationId: {CorrelationId}]",
            userId, pageSize, search, correlationId);

        var result = await _microsoftGraphService.ListFilesAsync(userId, skipToken, pageSize, search);

        return Ok(ApiResponse<MicrosoftFileListResponse>.Success(result, correlationId));
    }

    /// <summary>
    /// Retrieves metadata for a specific Microsoft OneDrive/SharePoint file.
    /// </summary>
    /// <param name="itemId">The OneDrive/SharePoint item ID.</param>
    /// <returns>The file metadata.</returns>
    [HttpGet("{itemId}")]
    [ProducesResponseType(typeof(ApiResponse<MicrosoftFileDto>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> GetFileMetadata([FromRoute] string itemId)
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userId = GetCurrentUserId();

        _logger.LogInformation(
            "Getting metadata for Microsoft file {ItemId} for user {UserId} [CorrelationId: {CorrelationId}]",
            itemId, userId, correlationId);

        var result = await _microsoftGraphService.GetFileMetadataAsync(userId, itemId);

        return Ok(ApiResponse<MicrosoftFileDto>.Success(result, correlationId));
    }

    /// <summary>
    /// Downloads the content of a specific Microsoft OneDrive/SharePoint file.
    /// Returns the raw file bytes with the appropriate Content-Type header.
    /// </summary>
    /// <param name="itemId">The OneDrive/SharePoint item ID.</param>
    /// <returns>The file content as a binary download.</returns>
    [HttpGet("{itemId}/download")]
    [ProducesResponseType(typeof(FileContentResult), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> DownloadFile([FromRoute] string itemId)
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userId = GetCurrentUserId();

        _logger.LogInformation(
            "Downloading Microsoft file {ItemId} for user {UserId} [CorrelationId: {CorrelationId}]",
            itemId, userId, correlationId);

        var result = await _microsoftGraphService.DownloadFileAsync(userId, itemId);

        return File(result.Content, result.MimeType, result.FileName);
    }

    /// <summary>
    /// Triggers a full sync of the authenticated user's OneDrive/SharePoint file metadata.
    /// Updates the local database and publishes indexing events for discovered files.
    /// </summary>
    /// <returns>A success response when the sync is complete.</returns>
    [HttpPost("sync")]
    [ProducesResponseType(typeof(ApiResponse), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> SyncFileMetadata()
    {
        var correlationId = HttpContext.Items["CorrelationId"] as string;
        var userId = GetCurrentUserId();

        _logger.LogInformation(
            "Starting Microsoft file metadata sync for user {UserId} [CorrelationId: {CorrelationId}]",
            userId, correlationId);

        await _microsoftGraphService.SyncFileMetadataAsync(userId);

        return Ok(ApiResponse.Success(correlationId));
    }

    /// <summary>
    /// Extracts the authenticated user's ID from the JWT claims.
    /// </summary>
    private Guid GetCurrentUserId()
    {
        var userIdClaim = User.FindFirst(ClaimTypes.NameIdentifier)?.Value
            ?? User.FindFirst("sub")?.Value;

        if (string.IsNullOrEmpty(userIdClaim) || !Guid.TryParse(userIdClaim, out var userId))
        {
            throw new UnauthorizedAccessException("Missing or invalid user ID in token.");
        }

        return userId;
    }
}
