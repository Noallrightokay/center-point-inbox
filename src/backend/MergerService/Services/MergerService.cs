using System.Net.Http.Headers;
using CenterPointInbox.MergerService.Models;
using CenterPointInbox.Shared.Contracts;
using MassTransit;
using Microsoft.EntityFrameworkCore;

namespace CenterPointInbox.MergerService.Services;

/// <summary>
/// Implementation of IMergerService. Queries the local files_metadata table for unified
/// file listings, applies filtering/sorting/pagination, and delegates file downloads
/// to the appropriate provider service via named HttpClients.
/// </summary>
public class MergerServiceImpl : IMergerService
{
    private readonly MergerDbContext _dbContext;
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly IPublishEndpoint _publishEndpoint;
    private readonly ILogger<MergerServiceImpl> _logger;

    private const int MaxPageSize = 100;

    public MergerServiceImpl(
        MergerDbContext dbContext,
        IHttpClientFactory httpClientFactory,
        IPublishEndpoint publishEndpoint,
        ILogger<MergerServiceImpl> logger)
    {
        _dbContext = dbContext;
        _httpClientFactory = httpClientFactory;
        _publishEndpoint = publishEndpoint;
        _logger = logger;
    }

    /// <inheritdoc />
    public async Task<UnifiedFileFeedResponse> GetUnifiedFeedAsync(Guid userId, Guid tenantId, FileFeedQuery query)
    {
        var pageSize = Math.Clamp(query.PageSize, 1, MaxPageSize);
        var page = Math.Max(query.Page, 1);

        var baseQuery = _dbContext.FilesMetadata
            .AsNoTracking()
            .Where(f => f.UserId == userId && f.TenantId == tenantId);

        // Apply provider filter
        if (!string.IsNullOrWhiteSpace(query.Provider))
        {
            baseQuery = baseQuery.Where(f => f.Provider.ToLower() == query.Provider.ToLower());
        }

        // Apply search filter (case-insensitive match on file name)
        if (!string.IsNullOrWhiteSpace(query.Search))
        {
            var searchTerm = query.Search.ToLower();
            baseQuery = baseQuery.Where(f => f.FileName.ToLower().Contains(searchTerm));
        }

        // Get total count before pagination
        var totalCount = await baseQuery.LongCountAsync();

        // Apply sorting
        baseQuery = ApplySorting(baseQuery, query.SortBy, query.SortDirection);

        // Apply pagination
        var skip = (page - 1) * pageSize;
        var files = await baseQuery
            .Skip(skip)
            .Take(pageSize)
            .ToListAsync();

        var fileDtos = files.Select(MapToUnifiedDto).ToList();

        _logger.LogInformation(
            "Unified feed returned {Count}/{Total} files for user {UserId} (page {Page}, provider={Provider}, search={Search})",
            fileDtos.Count, totalCount, userId, page, query.Provider ?? "all", query.Search ?? "none");

        await PublishAuditEventAsync(userId, tenantId, "ListUnifiedFeed", "FileFeed", "feed",
            new Dictionary<string, string>
            {
                ["page"] = page.ToString(),
                ["pageSize"] = pageSize.ToString(),
                ["totalCount"] = totalCount.ToString(),
                ["provider"] = query.Provider ?? "all",
                ["search"] = query.Search ?? string.Empty
            });

        return new UnifiedFileFeedResponse
        {
            Files = fileDtos,
            TotalCount = totalCount,
            Page = page,
            PageSize = pageSize,
            HasMore = (skip + pageSize) < totalCount
        };
    }

    /// <inheritdoc />
    public async Task<UnifiedFileDto> GetFileAsync(Guid userId, Guid tenantId, Guid fileId)
    {
        var file = await _dbContext.FilesMetadata
            .AsNoTracking()
            .FirstOrDefaultAsync(f => f.Id == fileId && f.UserId == userId && f.TenantId == tenantId)
            ?? throw new KeyNotFoundException($"File '{fileId}' not found.");

        _logger.LogInformation("Retrieved file metadata {FileId} for user {UserId}", fileId, userId);

        await PublishAuditEventAsync(userId, tenantId, "GetFileMetadata", "File", fileId.ToString());

        return MapToUnifiedDto(file);
    }

    /// <inheritdoc />
    public async Task<byte[]> DownloadFileAsync(Guid userId, Guid tenantId, Guid fileId)
    {
        var file = await _dbContext.FilesMetadata
            .AsNoTracking()
            .FirstOrDefaultAsync(f => f.Id == fileId && f.UserId == userId && f.TenantId == tenantId)
            ?? throw new KeyNotFoundException($"File '{fileId}' not found.");

        // Block download of quarantined files
        if (file.Quarantined)
        {
            _logger.LogWarning(
                "Download blocked for quarantined file {FileId} by user {UserId}",
                fileId, userId);
            throw new InvalidOperationException(
                $"File '{file.FileName}' is quarantined due to a policy violation and cannot be downloaded.");
        }

        // Determine which provider service to call
        var providerClient = ResolveProviderClient(file.Provider);
        var downloadUrl = BuildProviderDownloadUrl(file.Provider, file.ProviderFileId);

        _logger.LogInformation(
            "Downloading file {FileId} (provider={Provider}, providerFileId={ProviderFileId}) for user {UserId}",
            fileId, file.Provider, file.ProviderFileId, userId);

        var request = new HttpRequestMessage(HttpMethod.Get, downloadUrl);
        // Forward the user context so the provider service can authenticate the download
        request.Headers.Add("X-User-Id", userId.ToString());
        request.Headers.Add("X-Tenant-Id", tenantId.ToString());

        var response = await providerClient.SendAsync(request);

        if (!response.IsSuccessStatusCode)
        {
            var errorBody = await response.Content.ReadAsStringAsync();
            _logger.LogError(
                "Provider download failed for file {FileId}. Provider={Provider}, Status={StatusCode}, Body={Body}",
                fileId, file.Provider, response.StatusCode, errorBody);
            throw new HttpRequestException(
                $"Failed to download file from {file.Provider} provider. Status: {response.StatusCode}");
        }

        var content = await response.Content.ReadAsByteArrayAsync();

        await PublishAuditEventAsync(userId, tenantId, "DownloadFile", "File", fileId.ToString(),
            new Dictionary<string, string>
            {
                ["provider"] = file.Provider,
                ["fileName"] = file.FileName,
                ["mimeType"] = file.MimeType,
                ["sizeBytes"] = file.FileSizeBytes?.ToString() ?? "unknown"
            });

        return content;
    }

    /// <summary>
    /// Applies sorting to the file query based on the requested sort field and direction.
    /// </summary>
    private static IQueryable<FileMetadata> ApplySorting(
        IQueryable<FileMetadata> query, string sortBy, string sortDirection)
    {
        var descending = sortDirection.Equals("desc", StringComparison.OrdinalIgnoreCase);

        return sortBy.ToLowerInvariant() switch
        {
            "name" => descending
                ? query.OrderByDescending(f => f.FileName)
                : query.OrderBy(f => f.FileName),
            "size" => descending
                ? query.OrderByDescending(f => f.FileSizeBytes)
                : query.OrderBy(f => f.FileSizeBytes),
            "modified" or _ => descending
                ? query.OrderByDescending(f => f.LastModifiedProvider ?? f.UpdatedAt)
                : query.OrderBy(f => f.LastModifiedProvider ?? f.UpdatedAt),
        };
    }

    /// <summary>
    /// Maps a FileMetadata entity to a UnifiedFileDto for API consumers.
    /// </summary>
    private static UnifiedFileDto MapToUnifiedDto(FileMetadata file)
    {
        return new UnifiedFileDto
        {
            Id = file.Id,
            Provider = file.Provider,
            ProviderFileId = file.ProviderFileId,
            Name = file.FileName,
            MimeType = file.MimeType,
            SizeBytes = file.FileSizeBytes,
            ModifiedAt = file.LastModifiedProvider,
            WebUrl = file.ProviderUrl,
            IsShared = file.IsShared,
            PhiDetected = file.PhiDetected,
            Quarantined = file.Quarantined
        };
    }

    /// <summary>
    /// Resolves the named HttpClient for the given provider.
    /// </summary>
    private HttpClient ResolveProviderClient(string provider)
    {
        var clientName = provider.ToLowerInvariant() switch
        {
            "google" => "GoogleProvider",
            "microsoft" => "MicrosoftProvider",
            _ => throw new NotSupportedException($"Unknown file provider: '{provider}'.")
        };

        return _httpClientFactory.CreateClient(clientName);
    }

    /// <summary>
    /// Builds the download endpoint URL for the given provider and provider-specific file ID.
    /// </summary>
    private static string BuildProviderDownloadUrl(string provider, string providerFileId)
    {
        return provider.ToLowerInvariant() switch
        {
            "google" => $"api/files/google/{providerFileId}/download",
            "microsoft" => $"api/files/microsoft/{providerFileId}/download",
            _ => throw new NotSupportedException($"Unknown file provider: '{provider}'.")
        };
    }

    /// <summary>
    /// Publishes an audit event to the message bus. Failures are logged but do not
    /// interrupt the primary operation.
    /// </summary>
    private async Task PublishAuditEventAsync(
        Guid userId,
        Guid tenantId,
        string action,
        string resourceType,
        string resourceId,
        IDictionary<string, string>? metadata = null)
    {
        try
        {
            await _publishEndpoint.Publish(new AuditEvent
            {
                TenantId = tenantId,
                UserId = userId,
                Action = action,
                ResourceType = resourceType,
                ResourceId = resourceId,
                Metadata = metadata,
                OccurredAt = DateTimeOffset.UtcNow
            });
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex,
                "Failed to publish audit event for action {Action} on {ResourceType}/{ResourceId}",
                action, resourceType, resourceId);
        }
    }
}
