using CenterPointInbox.MergerService.Models;

namespace CenterPointInbox.MergerService.Services;

/// <summary>
/// Service interface for the unified file feed. Aggregates file metadata from
/// Google and Microsoft providers into a single, deduplicated, sorted view.
/// </summary>
public interface IMergerService
{
    /// <summary>
    /// Retrieves a paginated, filtered, and sorted unified file feed for a user.
    /// Queries the local files_metadata table which is populated by provider sync operations.
    /// </summary>
    /// <param name="userId">The authenticated user's ID.</param>
    /// <param name="tenantId">The user's tenant ID for multi-tenant isolation.</param>
    /// <param name="query">Filter, sort, and pagination parameters.</param>
    /// <returns>A paginated response containing unified file DTOs.</returns>
    Task<UnifiedFileFeedResponse> GetUnifiedFeedAsync(Guid userId, Guid tenantId, FileFeedQuery query);

    /// <summary>
    /// Retrieves metadata for a single file by its platform-level ID.
    /// </summary>
    /// <param name="userId">The authenticated user's ID.</param>
    /// <param name="tenantId">The user's tenant ID for multi-tenant isolation.</param>
    /// <param name="fileId">The platform file ID (files_metadata.id).</param>
    /// <returns>The unified file DTO.</returns>
    Task<UnifiedFileDto> GetFileAsync(Guid userId, Guid tenantId, Guid fileId);

    /// <summary>
    /// Downloads the binary content of a file by delegating to the appropriate
    /// provider service (Google or Microsoft) via HTTP. Checks quarantine status
    /// before allowing download and publishes an audit event.
    /// </summary>
    /// <param name="userId">The authenticated user's ID.</param>
    /// <param name="tenantId">The user's tenant ID for multi-tenant isolation.</param>
    /// <param name="fileId">The platform file ID (files_metadata.id).</param>
    /// <returns>The file content as a byte array.</returns>
    Task<byte[]> DownloadFileAsync(Guid userId, Guid tenantId, Guid fileId);
}
