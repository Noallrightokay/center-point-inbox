using System.Security.Claims;
using CenterPointInbox.GoogleProviderService.Models;
using CenterPointInbox.GoogleProviderService.Services;
using CenterPointInbox.Shared.Models;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

namespace CenterPointInbox.GoogleProviderService.Controllers;

/// <summary>
/// API controller for Google Drive file operations.
/// All endpoints require JWT authentication. The authenticated user's ID
/// is extracted from the token to scope all Drive operations.
/// </summary>
[ApiController]
[Route("api/files/google")]
[Authorize]
public class GoogleFilesController : ControllerBase
{
    private readonly IGoogleDriveService _driveService;
    private readonly ILogger<GoogleFilesController> _logger;

    public GoogleFilesController(IGoogleDriveService driveService, ILogger<GoogleFilesController> logger)
    {
        _driveService = driveService;
        _logger = logger;
    }

    /// <summary>
    /// Lists files from the authenticated user's Google Drive.
    /// Supports cursor-based pagination and Google Drive search queries.
    /// </summary>
    /// <param name="pageToken">Optional page token for pagination (returned from previous response).</param>
    /// <param name="pageSize">Number of results per page (default: 100, max: 1000).</param>
    /// <param name="query">Optional Google Drive search query (Drive API q parameter syntax).</param>
    /// <returns>A paginated list of Google Drive files.</returns>
    [HttpGet]
    [ProducesResponseType(typeof(ApiResponse<GoogleFileListResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> ListFiles(
        [FromQuery] string? pageToken = null,
        [FromQuery] int pageSize = 100,
        [FromQuery] string? query = null)
    {
        var userId = GetCurrentUserId();
        var correlationId = HttpContext.Items["CorrelationId"] as string;

        _logger.LogInformation(
            "Listing Google Drive files for user {UserId}. PageSize: {PageSize}, HasQuery: {HasQuery} [CorrelationId: {CorrelationId}]",
            userId, pageSize, !string.IsNullOrWhiteSpace(query), correlationId);

        var result = await _driveService.ListFilesAsync(userId, pageToken, pageSize, query);

        return Ok(ApiResponse<GoogleFileListResponse>.Success(result, correlationId));
    }

    /// <summary>
    /// Retrieves metadata for a specific file in the authenticated user's Google Drive.
    /// </summary>
    /// <param name="fileId">The Google Drive file ID.</param>
    /// <returns>The file metadata.</returns>
    [HttpGet("{fileId}")]
    [ProducesResponseType(typeof(ApiResponse<GoogleFileDto>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> GetFileMetadata([FromRoute] string fileId)
    {
        var userId = GetCurrentUserId();
        var correlationId = HttpContext.Items["CorrelationId"] as string;

        _logger.LogInformation(
            "Getting metadata for Google Drive file {FileId} for user {UserId} [CorrelationId: {CorrelationId}]",
            fileId, userId, correlationId);

        var result = await _driveService.GetFileMetadataAsync(userId, fileId);

        return Ok(ApiResponse<GoogleFileDto>.Success(result, correlationId));
    }

    /// <summary>
    /// Downloads the binary content of a file from the authenticated user's Google Drive.
    /// Returns the file as a downloadable attachment. For Google Workspace files (Docs, Sheets, Slides),
    /// use the export endpoint instead.
    /// </summary>
    /// <param name="fileId">The Google Drive file ID.</param>
    /// <returns>The file content as a binary download.</returns>
    [HttpGet("{fileId}/download")]
    [ProducesResponseType(typeof(FileContentResult), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status400BadRequest)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> DownloadFile([FromRoute] string fileId)
    {
        var userId = GetCurrentUserId();
        var correlationId = HttpContext.Items["CorrelationId"] as string;

        _logger.LogInformation(
            "Downloading Google Drive file {FileId} for user {UserId} [CorrelationId: {CorrelationId}]",
            fileId, userId, correlationId);

        var result = await _driveService.DownloadFileAsync(userId, fileId);

        return File(result.Content, result.MimeType, result.FileName);
    }

    /// <summary>
    /// Exports a Google Workspace file (Docs, Sheets, Slides) to the specified MIME type.
    /// Returns the exported file as a downloadable attachment.
    /// </summary>
    /// <param name="fileId">The Google Drive file ID.</param>
    /// <param name="mimeType">The target export MIME type (e.g., application/pdf, text/plain).</param>
    /// <returns>The exported file content as a binary download.</returns>
    [HttpGet("{fileId}/export")]
    [ProducesResponseType(typeof(FileContentResult), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status400BadRequest)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> ExportFile([FromRoute] string fileId, [FromQuery] string mimeType)
    {
        var userId = GetCurrentUserId();
        var correlationId = HttpContext.Items["CorrelationId"] as string;

        if (string.IsNullOrWhiteSpace(mimeType))
        {
            return BadRequest(ApiResponse<object>.Error(
                "The 'mimeType' query parameter is required for export.",
                "MISSING_MIME_TYPE", StatusCodes.Status400BadRequest, correlationId));
        }

        _logger.LogInformation(
            "Exporting Google Drive file {FileId} as {MimeType} for user {UserId} [CorrelationId: {CorrelationId}]",
            fileId, mimeType, userId, correlationId);

        var result = await _driveService.ExportFileAsync(userId, fileId, mimeType);

        return File(result.Content, result.MimeType, result.FileName);
    }

    /// <summary>
    /// Triggers a full sync of file metadata from the authenticated user's Google Drive
    /// into the local database. New files will be queued for indexing.
    /// </summary>
    /// <returns>A success response once the sync completes.</returns>
    [HttpPost("sync")]
    [ProducesResponseType(typeof(ApiResponse), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> SyncFiles()
    {
        var userId = GetCurrentUserId();
        var correlationId = HttpContext.Items["CorrelationId"] as string;

        _logger.LogInformation(
            "Starting Google Drive sync for user {UserId} [CorrelationId: {CorrelationId}]",
            userId, correlationId);

        await _driveService.SyncFileMetadataAsync(userId);

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
