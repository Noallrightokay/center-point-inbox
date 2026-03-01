using CenterPointInbox.GoogleProviderService.Models;

namespace CenterPointInbox.GoogleProviderService.Services;

/// <summary>
/// Service interface for interacting with the Google Drive API on behalf of users.
/// All operations use the authenticated user's OAuth token retrieved from the database.
/// </summary>
public interface IGoogleDriveService
{
    /// <summary>
    /// Lists files from the user's Google Drive with optional pagination and search query.
    /// </summary>
    /// <param name="userId">The platform user ID whose Google Drive to query.</param>
    /// <param name="pageToken">Optional page token for cursor-based pagination.</param>
    /// <param name="pageSize">Number of results per page.</param>
    /// <param name="query">Optional Google Drive search query (uses Drive API q parameter syntax).</param>
    /// <returns>A paginated list of file DTOs with an optional next page token.</returns>
    Task<GoogleFileListResponse> ListFilesAsync(Guid userId, string? pageToken, int pageSize, string? query);

    /// <summary>
    /// Retrieves metadata for a single file from the user's Google Drive.
    /// </summary>
    /// <param name="userId">The platform user ID whose Google Drive to query.</param>
    /// <param name="fileId">The Google Drive file ID.</param>
    /// <returns>File metadata DTO.</returns>
    Task<GoogleFileDto> GetFileMetadataAsync(Guid userId, string fileId);

    /// <summary>
    /// Downloads the binary content of a file from the user's Google Drive.
    /// For Google Workspace files (Docs, Sheets, Slides), use ExportFileAsync instead.
    /// </summary>
    /// <param name="userId">The platform user ID whose Google Drive to query.</param>
    /// <param name="fileId">The Google Drive file ID.</param>
    /// <returns>The file content, MIME type, and file name.</returns>
    Task<FileContentResponse> DownloadFileAsync(Guid userId, string fileId);

    /// <summary>
    /// Exports a Google Workspace file (Docs, Sheets, Slides) to the specified MIME type.
    /// </summary>
    /// <param name="userId">The platform user ID whose Google Drive to query.</param>
    /// <param name="fileId">The Google Drive file ID.</param>
    /// <param name="exportMimeType">The target MIME type (e.g., application/pdf, text/plain).</param>
    /// <returns>The exported file content, MIME type, and file name.</returns>
    Task<FileContentResponse> ExportFileAsync(Guid userId, string fileId, string exportMimeType);

    /// <summary>
    /// Syncs file metadata from the user's Google Drive into the local FilesMetadata table.
    /// Performs upserts based on (userId, provider, providerFileId) and publishes
    /// FileIndexRequested events for newly discovered files.
    /// </summary>
    /// <param name="userId">The platform user ID whose Google Drive to sync.</param>
    Task SyncFileMetadataAsync(Guid userId);
}
