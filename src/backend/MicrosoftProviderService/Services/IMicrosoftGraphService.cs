using CenterPointInbox.MicrosoftProviderService.Models;

namespace CenterPointInbox.MicrosoftProviderService.Services;

/// <summary>
/// Service interface for interacting with the Microsoft Graph API
/// to manage OneDrive/SharePoint files on behalf of users.
/// </summary>
public interface IMicrosoftGraphService
{
    /// <summary>
    /// Lists files from the user's OneDrive with pagination and optional search.
    /// </summary>
    /// <param name="userId">The Center Point user ID.</param>
    /// <param name="skipToken">Optional skip token for pagination continuation.</param>
    /// <param name="pageSize">Number of files per page.</param>
    /// <param name="search">Optional search query to filter files by name.</param>
    /// <returns>A paginated list of Microsoft file DTOs.</returns>
    Task<MicrosoftFileListResponse> ListFilesAsync(Guid userId, string? skipToken, int pageSize, string? search);

    /// <summary>
    /// Retrieves metadata for a specific file by its OneDrive/SharePoint item ID.
    /// </summary>
    /// <param name="userId">The Center Point user ID.</param>
    /// <param name="itemId">The OneDrive/SharePoint item ID.</param>
    /// <returns>The file metadata DTO.</returns>
    Task<MicrosoftFileDto> GetFileMetadataAsync(Guid userId, string itemId);

    /// <summary>
    /// Downloads the content of a specific file from OneDrive/SharePoint.
    /// </summary>
    /// <param name="userId">The Center Point user ID.</param>
    /// <param name="itemId">The OneDrive/SharePoint item ID.</param>
    /// <returns>The file content, MIME type, and file name.</returns>
    Task<FileContentResponse> DownloadFileAsync(Guid userId, string itemId);

    /// <summary>
    /// Synchronizes the user's OneDrive/SharePoint file metadata into the local database
    /// and publishes indexing events for new/updated files.
    /// </summary>
    /// <param name="userId">The Center Point user ID.</param>
    Task SyncFileMetadataAsync(Guid userId);
}
