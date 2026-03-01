using System.Net.Http.Headers;
using System.Text.Json;
using CenterPointInbox.MicrosoftProviderService.Configuration;
using CenterPointInbox.MicrosoftProviderService.Models;
using CenterPointInbox.Shared.Contracts;
using CenterPointInbox.Shared.Security;
using MassTransit;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Options;
using Microsoft.Graph;
using Microsoft.Graph.Models;
using Microsoft.Kiota.Abstractions.Authentication;

namespace CenterPointInbox.MicrosoftProviderService.Services;

/// <summary>
/// Implementation of IMicrosoftGraphService that uses the Microsoft Graph SDK
/// to interact with OneDrive/SharePoint files. Handles OAuth token decryption,
/// token refresh, file operations, metadata sync, and event publishing.
/// </summary>
public class MicrosoftGraphService : IMicrosoftGraphService
{
    private readonly MicrosoftDbContext _dbContext;
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly IPublishEndpoint _publishEndpoint;
    private readonly MicrosoftSettings _settings;
    private readonly ILogger<MicrosoftGraphService> _logger;

    public MicrosoftGraphService(
        MicrosoftDbContext dbContext,
        IHttpClientFactory httpClientFactory,
        IPublishEndpoint publishEndpoint,
        IOptions<MicrosoftSettings> settings,
        ILogger<MicrosoftGraphService> logger)
    {
        _dbContext = dbContext;
        _httpClientFactory = httpClientFactory;
        _publishEndpoint = publishEndpoint;
        _settings = settings.Value;
        _logger = logger;
    }

    /// <inheritdoc />
    public async Task<MicrosoftFileListResponse> ListFilesAsync(
        Guid userId,
        string? skipToken,
        int pageSize,
        string? search)
    {
        var graphClient = await CreateGraphClientAsync(userId);

        pageSize = Math.Clamp(pageSize, 1, _settings.MaxPageSize);

        DriveItemCollectionResponse? driveItems;

        if (!string.IsNullOrWhiteSpace(search))
        {
            // Use Graph search endpoint for OneDrive
            var searchResults = await graphClient.Me.Drive.Root
                .SearchWithQ(search)
                .GetAsync(config =>
                {
                    config.QueryParameters.Top = pageSize;
                    if (!string.IsNullOrWhiteSpace(skipToken))
                    {
                        config.QueryParameters.Skip = int.TryParse(skipToken, out var skip) ? skip : 0;
                    }
                    config.QueryParameters.Select = new[]
                    {
                        "id", "name", "file", "size", "lastModifiedDateTime",
                        "createdBy", "webUrl", "parentReference", "shared"
                    };
                });

            var files = (searchResults?.Value ?? [])
                .Where(item => item.File != null)
                .Select(MapDriveItemToDto)
                .ToList();

            return new MicrosoftFileListResponse
            {
                Files = files,
                NextLink = searchResults?.OdataNextLink
            };
        }

        driveItems = await graphClient.Me.Drive.Root.Children
            .GetAsync(config =>
            {
                config.QueryParameters.Top = pageSize;
                config.QueryParameters.Select = new[]
                {
                    "id", "name", "file", "folder", "size", "lastModifiedDateTime",
                    "createdBy", "webUrl", "parentReference", "shared"
                };
                if (!string.IsNullOrWhiteSpace(skipToken))
                {
                    config.QueryParameters.Skip = int.TryParse(skipToken, out var skip) ? skip : 0;
                }
                config.QueryParameters.Orderby = new[] { "name asc" };
            });

        var fileList = (driveItems?.Value ?? [])
            .Where(item => item.File != null)
            .Select(MapDriveItemToDto)
            .ToList();

        return new MicrosoftFileListResponse
        {
            Files = fileList,
            NextLink = driveItems?.OdataNextLink
        };
    }

    /// <inheritdoc />
    public async Task<MicrosoftFileDto> GetFileMetadataAsync(Guid userId, string itemId)
    {
        ArgumentException.ThrowIfNullOrEmpty(itemId);

        var graphClient = await CreateGraphClientAsync(userId);

        var driveItem = await graphClient.Me.Drive.Items[itemId]
            .GetAsync(config =>
            {
                config.QueryParameters.Select = new[]
                {
                    "id", "name", "file", "size", "lastModifiedDateTime",
                    "createdBy", "webUrl", "parentReference", "shared"
                };
            });

        if (driveItem is null)
        {
            throw new KeyNotFoundException($"File with item ID '{itemId}' was not found.");
        }

        return MapDriveItemToDto(driveItem);
    }

    /// <inheritdoc />
    public async Task<FileContentResponse> DownloadFileAsync(Guid userId, string itemId)
    {
        ArgumentException.ThrowIfNullOrEmpty(itemId);

        var graphClient = await CreateGraphClientAsync(userId);

        // Get file metadata first for name and MIME type
        var driveItem = await graphClient.Me.Drive.Items[itemId]
            .GetAsync(config =>
            {
                config.QueryParameters.Select = new[] { "id", "name", "file", "size" };
            });

        if (driveItem is null)
        {
            throw new KeyNotFoundException($"File with item ID '{itemId}' was not found.");
        }

        // Download the file content stream
        var contentStream = await graphClient.Me.Drive.Items[itemId].Content
            .GetAsync();

        if (contentStream is null)
        {
            throw new InvalidOperationException($"Failed to download content for file '{itemId}'.");
        }

        using var memoryStream = new MemoryStream();
        await contentStream.CopyToAsync(memoryStream);

        var mimeType = driveItem.File?.MimeType ?? "application/octet-stream";
        var fileName = driveItem.Name ?? $"file_{itemId}";

        return new FileContentResponse
        {
            Content = memoryStream.ToArray(),
            MimeType = mimeType,
            FileName = fileName
        };
    }

    /// <inheritdoc />
    public async Task SyncFileMetadataAsync(Guid userId)
    {
        _logger.LogInformation("Starting file metadata sync for user {UserId}", userId);

        var graphClient = await CreateGraphClientAsync(userId);
        var connection = await GetOAuthConnectionAsync(userId);
        var allFiles = new List<DriveItem>();

        // Paginate through all files in OneDrive
        var response = await graphClient.Me.Drive.Root.Children
            .GetAsync(config =>
            {
                config.QueryParameters.Top = _settings.DefaultPageSize;
                config.QueryParameters.Select = new[]
                {
                    "id", "name", "file", "folder", "size", "lastModifiedDateTime",
                    "createdBy", "webUrl", "parentReference", "shared"
                };
            });

        while (response != null)
        {
            var items = response.Value ?? [];
            allFiles.AddRange(items.Where(i => i.File != null));

            if (string.IsNullOrEmpty(response.OdataNextLink))
            {
                break;
            }

            response = await graphClient.Me.Drive.Root.Children
                .WithUrl(response.OdataNextLink)
                .GetAsync();
        }

        _logger.LogInformation(
            "Fetched {FileCount} files from OneDrive for user {UserId}",
            allFiles.Count, userId);

        // Upsert file metadata into the database
        foreach (var driveItem in allFiles)
        {
            var providerFileId = driveItem.Id ?? string.Empty;

            var existing = await _dbContext.FilesMetadata
                .FirstOrDefaultAsync(f =>
                    f.UserId == userId &&
                    f.Provider == "Microsoft" &&
                    f.ProviderFileId == providerFileId);

            if (existing is not null)
            {
                existing.FileName = driveItem.Name ?? existing.FileName;
                existing.MimeType = driveItem.File?.MimeType;
                existing.Size = driveItem.Size;
                existing.WebUrl = driveItem.WebUrl;
                existing.ParentPath = driveItem.ParentReference?.Path;
                existing.IsShared = driveItem.Shared != null;
                existing.LastModifiedAt = driveItem.LastModifiedDateTime;
                existing.SyncedAt = DateTimeOffset.UtcNow;
            }
            else
            {
                var newFile = new FileMetadataEntity
                {
                    Id = Guid.NewGuid(),
                    UserId = userId,
                    Provider = "Microsoft",
                    ProviderFileId = providerFileId,
                    FileName = driveItem.Name ?? string.Empty,
                    MimeType = driveItem.File?.MimeType,
                    Size = driveItem.Size,
                    WebUrl = driveItem.WebUrl,
                    ParentPath = driveItem.ParentReference?.Path,
                    IsShared = driveItem.Shared != null,
                    LastModifiedAt = driveItem.LastModifiedDateTime,
                    SyncedAt = DateTimeOffset.UtcNow,
                    CreatedAt = DateTimeOffset.UtcNow
                };

                _dbContext.FilesMetadata.Add(newFile);
            }
        }

        await _dbContext.SaveChangesAsync();

        // Publish FileIndexRequested events for all synced files
        var syncedFiles = await _dbContext.FilesMetadata
            .Where(f => f.UserId == userId && f.Provider == "Microsoft")
            .ToListAsync();

        foreach (var file in syncedFiles)
        {
            await _publishEndpoint.Publish(new FileIndexRequested
            {
                TenantId = await GetTenantIdForUserAsync(userId),
                UserId = userId,
                FileId = file.Id,
                Provider = "Microsoft",
                RequestedAt = DateTimeOffset.UtcNow
            });
        }

        // Publish audit event for the sync operation
        await _publishEndpoint.Publish(new AuditEvent
        {
            TenantId = await GetTenantIdForUserAsync(userId),
            UserId = userId,
            Action = "FileMetadataSync",
            ResourceType = "MicrosoftDrive",
            ResourceId = userId.ToString(),
            Metadata = new Dictionary<string, string>
            {
                ["fileCount"] = allFiles.Count.ToString(),
                ["provider"] = "Microsoft"
            },
            OccurredAt = DateTimeOffset.UtcNow
        });

        _logger.LogInformation(
            "Completed file metadata sync for user {UserId}. Synced {FileCount} files.",
            userId, allFiles.Count);
    }

    /// <summary>
    /// Creates a GraphServiceClient authenticated with the user's decrypted access token.
    /// Handles token refresh if the current token has expired.
    /// </summary>
    private async Task<GraphServiceClient> CreateGraphClientAsync(Guid userId)
    {
        var connection = await GetOAuthConnectionAsync(userId);
        var accessToken = await GetValidAccessTokenAsync(connection);

        var authProvider = new BaseBearerTokenAuthenticationProvider(
            new TokenProvider(accessToken));

        return new GraphServiceClient(authProvider);
    }

    /// <summary>
    /// Retrieves the Microsoft OAuth connection for the given user.
    /// </summary>
    private async Task<OAuthConnectionEntity> GetOAuthConnectionAsync(Guid userId)
    {
        var connection = await _dbContext.OAuthConnections
            .FirstOrDefaultAsync(o =>
                o.UserId == userId &&
                o.Provider == OAuthProvider.Microsoft);

        if (connection is null)
        {
            throw new KeyNotFoundException(
                $"No Microsoft OAuth connection found for user '{userId}'. " +
                "Please connect your Microsoft account first.");
        }

        return connection;
    }

    /// <summary>
    /// Returns a valid access token, refreshing it if necessary.
    /// Decrypts the stored token using the shared TokenEncryption utility.
    /// </summary>
    private async Task<string> GetValidAccessTokenAsync(OAuthConnectionEntity connection)
    {
        // Check if the access token is still valid (with a 5-minute buffer)
        if (connection.AccessTokenExpiresAt.HasValue &&
            connection.AccessTokenExpiresAt.Value > DateTimeOffset.UtcNow.AddMinutes(5))
        {
            return TokenEncryption.Decrypt(connection.EncryptedAccessToken, _settings.TokenEncryptionKey);
        }

        // Token is expired or about to expire; attempt refresh
        if (string.IsNullOrEmpty(connection.EncryptedRefreshToken))
        {
            throw new UnauthorizedAccessException(
                "Microsoft access token has expired and no refresh token is available. " +
                "Please re-authenticate with Microsoft.");
        }

        _logger.LogInformation(
            "Access token expired for user {UserId}. Attempting refresh.",
            connection.UserId);

        var refreshToken = TokenEncryption.Decrypt(
            connection.EncryptedRefreshToken,
            _settings.TokenEncryptionKey);

        var newTokens = await RefreshAccessTokenAsync(refreshToken);

        // Update the connection in the database with the new tokens
        connection.EncryptedAccessToken = TokenEncryption.Encrypt(
            newTokens.AccessToken,
            _settings.TokenEncryptionKey);

        if (!string.IsNullOrEmpty(newTokens.RefreshToken))
        {
            connection.EncryptedRefreshToken = TokenEncryption.Encrypt(
                newTokens.RefreshToken,
                _settings.TokenEncryptionKey);
        }

        connection.AccessTokenExpiresAt = DateTimeOffset.UtcNow.AddSeconds(newTokens.ExpiresIn);
        connection.UpdatedAt = DateTimeOffset.UtcNow;

        await _dbContext.SaveChangesAsync();

        _logger.LogInformation(
            "Successfully refreshed access token for user {UserId}",
            connection.UserId);

        return newTokens.AccessToken;
    }

    /// <summary>
    /// Performs an OAuth token refresh against the Microsoft token endpoint.
    /// </summary>
    private async Task<TokenRefreshResult> RefreshAccessTokenAsync(string refreshToken)
    {
        var httpClient = _httpClientFactory.CreateClient("MicrosoftOAuth");

        var requestBody = new FormUrlEncodedContent(new Dictionary<string, string>
        {
            ["client_id"] = _settings.ClientId,
            ["client_secret"] = _settings.ClientSecret,
            ["refresh_token"] = refreshToken,
            ["grant_type"] = "refresh_token",
            ["scope"] = "https://graph.microsoft.com/.default offline_access"
        });

        var response = await httpClient.PostAsync(_settings.TokenEndpoint, requestBody);

        if (!response.IsSuccessStatusCode)
        {
            var errorContent = await response.Content.ReadAsStringAsync();
            _logger.LogError(
                "Failed to refresh Microsoft access token. Status: {StatusCode}, Response: {Response}",
                response.StatusCode, errorContent);

            throw new UnauthorizedAccessException(
                "Failed to refresh Microsoft access token. Please re-authenticate with Microsoft.");
        }

        var responseContent = await response.Content.ReadAsStringAsync();
        var tokenResponse = JsonSerializer.Deserialize<MicrosoftTokenResponse>(responseContent);

        if (tokenResponse is null || string.IsNullOrEmpty(tokenResponse.AccessToken))
        {
            throw new InvalidOperationException("Received an invalid token response from Microsoft.");
        }

        return new TokenRefreshResult
        {
            AccessToken = tokenResponse.AccessToken,
            RefreshToken = tokenResponse.RefreshToken,
            ExpiresIn = tokenResponse.ExpiresIn
        };
    }

    /// <summary>
    /// Maps a Microsoft Graph DriveItem to a MicrosoftFileDto.
    /// </summary>
    private static MicrosoftFileDto MapDriveItemToDto(DriveItem item)
    {
        return new MicrosoftFileDto
        {
            Id = item.Id ?? string.Empty,
            Name = item.Name ?? string.Empty,
            MimeType = item.File?.MimeType ?? "application/octet-stream",
            Size = item.Size,
            LastModifiedDateTime = item.LastModifiedDateTime,
            CreatedBy = item.CreatedBy?.User?.DisplayName,
            WebUrl = item.WebUrl,
            ParentPath = item.ParentReference?.Path,
            Shared = item.Shared != null
        };
    }

    /// <summary>
    /// Retrieves the tenant ID for a given user by querying the users table.
    /// Falls back to Guid.Empty if the user is not found.
    /// </summary>
    private async Task<Guid> GetTenantIdForUserAsync(Guid userId)
    {
        // Query the users table to get the tenant ID
        var tenantId = await _dbContext.Database
            .SqlQueryRaw<Guid>(
                "SELECT tenant_id AS \"Value\" FROM users WHERE id = {0} LIMIT 1",
                userId)
            .FirstOrDefaultAsync();

        return tenantId;
    }

    /// <summary>
    /// Internal DTO for Microsoft token refresh response.
    /// </summary>
    private sealed class MicrosoftTokenResponse
    {
        [System.Text.Json.Serialization.JsonPropertyName("access_token")]
        public string AccessToken { get; set; } = string.Empty;

        [System.Text.Json.Serialization.JsonPropertyName("refresh_token")]
        public string? RefreshToken { get; set; }

        [System.Text.Json.Serialization.JsonPropertyName("expires_in")]
        public int ExpiresIn { get; set; }

        [System.Text.Json.Serialization.JsonPropertyName("token_type")]
        public string TokenType { get; set; } = string.Empty;
    }

    /// <summary>
    /// Internal result type for token refresh operations.
    /// </summary>
    private sealed class TokenRefreshResult
    {
        public string AccessToken { get; init; } = string.Empty;
        public string? RefreshToken { get; init; }
        public int ExpiresIn { get; init; }
    }

    /// <summary>
    /// Simple token provider that supplies a static access token to the Graph SDK.
    /// Implements IAccessTokenProvider for the BaseBearerTokenAuthenticationProvider.
    /// </summary>
    private sealed class TokenProvider : IAccessTokenProvider
    {
        private readonly string _accessToken;

        public TokenProvider(string accessToken)
        {
            _accessToken = accessToken;
        }

        public Task<string> GetAuthorizationTokenAsync(
            Uri uri,
            Dictionary<string, object>? additionalAuthenticationContext = null,
            CancellationToken cancellationToken = default)
        {
            return Task.FromResult(_accessToken);
        }

        public AllowedHostsValidator AllowedHostsValidator { get; } = new();
    }
}
