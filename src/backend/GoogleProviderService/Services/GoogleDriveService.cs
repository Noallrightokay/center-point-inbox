using System.Text.Json;
using CenterPointInbox.GoogleProviderService.Configuration;
using CenterPointInbox.GoogleProviderService.Models;
using CenterPointInbox.Shared.Contracts;
using CenterPointInbox.Shared.Security;
using Google.Apis.Auth.OAuth2;
using Google.Apis.Drive.v3;
using Google.Apis.Services;
using MassTransit;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Options;

namespace CenterPointInbox.GoogleProviderService.Services;

/// <summary>
/// Implementation of IGoogleDriveService that communicates with the Google Drive API v3.
/// Reads encrypted OAuth tokens from the database, decrypts them, handles token refresh,
/// and maps Google API responses to platform DTOs.
/// </summary>
public class GoogleDriveService : IGoogleDriveService
{
    private readonly GoogleDbContext _dbContext;
    private readonly GoogleSettings _settings;
    private readonly IPublishEndpoint _publishEndpoint;
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly ILogger<GoogleDriveService> _logger;

    /// <summary>
    /// Standard fields to request from the Google Drive API for file metadata.
    /// </summary>
    private const string FileFields = "id, name, mimeType, size, modifiedTime, owners(displayName, emailAddress), shared, webViewLink, thumbnailLink, parents";

    /// <summary>
    /// Fields to request for file listing responses (includes pagination token).
    /// </summary>
    private const string ListFields = $"nextPageToken, files({FileFields})";

    public GoogleDriveService(
        GoogleDbContext dbContext,
        IOptions<GoogleSettings> settings,
        IPublishEndpoint publishEndpoint,
        IHttpClientFactory httpClientFactory,
        ILogger<GoogleDriveService> logger)
    {
        _dbContext = dbContext;
        _settings = settings.Value;
        _publishEndpoint = publishEndpoint;
        _httpClientFactory = httpClientFactory;
        _logger = logger;
    }

    /// <inheritdoc />
    public async Task<GoogleFileListResponse> ListFilesAsync(Guid userId, string? pageToken, int pageSize, string? query)
    {
        var driveService = await CreateDriveServiceAsync(userId);

        var listRequest = driveService.Files.List();
        listRequest.PageSize = Math.Clamp(pageSize, 1, _settings.MaxPageSize);
        listRequest.Fields = ListFields;
        listRequest.OrderBy = "modifiedTime desc";

        if (!string.IsNullOrWhiteSpace(pageToken))
        {
            listRequest.PageToken = pageToken;
        }

        if (!string.IsNullOrWhiteSpace(query))
        {
            listRequest.Q = query;
        }

        var result = await listRequest.ExecuteAsync();

        await PublishAuditEventAsync(userId, "ListFiles", "GoogleDrive", "drive",
            new Dictionary<string, string>
            {
                ["pageSize"] = pageSize.ToString(),
                ["hasQuery"] = (!string.IsNullOrWhiteSpace(query)).ToString()
            });

        return new GoogleFileListResponse
        {
            Files = result.Files?.Select(MapToDto).ToList() ?? [],
            NextPageToken = result.NextPageToken
        };
    }

    /// <inheritdoc />
    public async Task<GoogleFileDto> GetFileMetadataAsync(Guid userId, string fileId)
    {
        ArgumentException.ThrowIfNullOrEmpty(fileId);

        var driveService = await CreateDriveServiceAsync(userId);

        var getRequest = driveService.Files.Get(fileId);
        getRequest.Fields = FileFields;

        var file = await getRequest.ExecuteAsync();

        await PublishAuditEventAsync(userId, "GetFileMetadata", "GoogleDriveFile", fileId);

        return MapToDto(file);
    }

    /// <inheritdoc />
    public async Task<FileContentResponse> DownloadFileAsync(Guid userId, string fileId)
    {
        ArgumentException.ThrowIfNullOrEmpty(fileId);

        var driveService = await CreateDriveServiceAsync(userId);

        // First, get file metadata to determine MIME type and name
        var metaRequest = driveService.Files.Get(fileId);
        metaRequest.Fields = "id, name, mimeType, size";
        var fileMeta = await metaRequest.ExecuteAsync();

        // Google Workspace files cannot be downloaded directly; they must be exported
        if (IsGoogleWorkspaceFile(fileMeta.MimeType))
        {
            throw new NotSupportedException(
                $"File '{fileMeta.Name}' is a Google Workspace file ({fileMeta.MimeType}). " +
                "Use the export endpoint instead with a target MIME type.");
        }

        var downloadRequest = driveService.Files.Get(fileId);
        using var stream = new MemoryStream();
        var progress = await downloadRequest.DownloadAsync(stream);

        if (progress.Exception != null)
        {
            _logger.LogError(progress.Exception, "Failed to download file {FileId} for user {UserId}", fileId, userId);
            throw new InvalidOperationException($"Failed to download file '{fileId}': {progress.Exception.Message}", progress.Exception);
        }

        await PublishAuditEventAsync(userId, "DownloadFile", "GoogleDriveFile", fileId,
            new Dictionary<string, string>
            {
                ["fileName"] = fileMeta.Name,
                ["mimeType"] = fileMeta.MimeType,
                ["size"] = fileMeta.Size?.ToString() ?? "unknown"
            });

        return new FileContentResponse
        {
            Content = stream.ToArray(),
            MimeType = fileMeta.MimeType,
            FileName = fileMeta.Name
        };
    }

    /// <inheritdoc />
    public async Task<FileContentResponse> ExportFileAsync(Guid userId, string fileId, string exportMimeType)
    {
        ArgumentException.ThrowIfNullOrEmpty(fileId);
        ArgumentException.ThrowIfNullOrEmpty(exportMimeType);

        var driveService = await CreateDriveServiceAsync(userId);

        // Get file metadata for the name
        var metaRequest = driveService.Files.Get(fileId);
        metaRequest.Fields = "id, name, mimeType";
        var fileMeta = await metaRequest.ExecuteAsync();

        if (!IsGoogleWorkspaceFile(fileMeta.MimeType))
        {
            throw new NotSupportedException(
                $"File '{fileMeta.Name}' ({fileMeta.MimeType}) is not a Google Workspace file. " +
                "Use the download endpoint instead.");
        }

        var exportRequest = driveService.Files.Export(fileId, exportMimeType);
        using var stream = new MemoryStream();
        var progress = await exportRequest.DownloadAsync(stream);

        if (progress.Exception != null)
        {
            _logger.LogError(progress.Exception, "Failed to export file {FileId} as {MimeType} for user {UserId}",
                fileId, exportMimeType, userId);
            throw new InvalidOperationException(
                $"Failed to export file '{fileId}' as '{exportMimeType}': {progress.Exception.Message}",
                progress.Exception);
        }

        var extension = GetExtensionForMimeType(exportMimeType);
        var exportFileName = Path.GetFileNameWithoutExtension(fileMeta.Name) + extension;

        await PublishAuditEventAsync(userId, "ExportFile", "GoogleDriveFile", fileId,
            new Dictionary<string, string>
            {
                ["fileName"] = fileMeta.Name,
                ["sourceMimeType"] = fileMeta.MimeType,
                ["exportMimeType"] = exportMimeType
            });

        return new FileContentResponse
        {
            Content = stream.ToArray(),
            MimeType = exportMimeType,
            FileName = exportFileName
        };
    }

    /// <inheritdoc />
    public async Task SyncFileMetadataAsync(Guid userId)
    {
        _logger.LogInformation("Starting file metadata sync for user {UserId}", userId);

        var driveService = await CreateDriveServiceAsync(userId);
        var newFileIds = new List<Guid>();
        string? pageToken = null;

        do
        {
            var listRequest = driveService.Files.List();
            listRequest.PageSize = _settings.DefaultPageSize;
            listRequest.Fields = ListFields;
            listRequest.OrderBy = "modifiedTime desc";

            if (!string.IsNullOrWhiteSpace(pageToken))
            {
                listRequest.PageToken = pageToken;
            }

            var result = await listRequest.ExecuteAsync();

            if (result.Files != null)
            {
                foreach (var file in result.Files)
                {
                    var newFileId = await UpsertFileMetadataAsync(userId, file);
                    if (newFileId.HasValue)
                    {
                        newFileIds.Add(newFileId.Value);
                    }
                }
            }

            pageToken = result.NextPageToken;

        } while (!string.IsNullOrWhiteSpace(pageToken));

        await _dbContext.SaveChangesAsync();

        // Publish FileIndexRequested events for newly discovered files
        foreach (var fileId in newFileIds)
        {
            await _publishEndpoint.Publish(new FileIndexRequested
            {
                UserId = userId,
                FileId = fileId,
                Provider = "Google",
                RequestedAt = DateTimeOffset.UtcNow
            });
        }

        _logger.LogInformation(
            "File metadata sync completed for user {UserId}. New files discovered: {NewFileCount}",
            userId, newFileIds.Count);

        await PublishAuditEventAsync(userId, "SyncFileMetadata", "GoogleDrive", "drive",
            new Dictionary<string, string>
            {
                ["newFilesCount"] = newFileIds.Count.ToString()
            });
    }

    /// <summary>
    /// Upserts a Google Drive file into the FilesMetadata table.
    /// Returns the new FileMetadata ID if the file was inserted (not updated), or null if it was an update.
    /// </summary>
    private async Task<Guid?> UpsertFileMetadataAsync(Guid userId, Google.Apis.Drive.v3.Data.File file)
    {
        var existing = await _dbContext.FilesMetadata
            .FirstOrDefaultAsync(f =>
                f.UserId == userId &&
                f.Provider == "Google" &&
                f.ProviderFileId == file.Id);

        var owners = file.Owners != null
            ? string.Join(";", file.Owners.Select(o => o.EmailAddress ?? o.DisplayName))
            : null;

        var parentId = file.Parents?.FirstOrDefault();

        if (existing != null)
        {
            existing.FileName = file.Name;
            existing.MimeType = file.MimeType;
            existing.FileSize = file.Size;
            existing.ProviderModifiedAt = file.ModifiedTimeDateTimeOffset;
            existing.WebViewLink = file.WebViewLink;
            existing.ThumbnailLink = file.ThumbnailLink;
            existing.ParentId = parentId;
            existing.IsShared = file.Shared ?? false;
            existing.Owners = owners;
            existing.SyncedAt = DateTimeOffset.UtcNow;
            existing.UpdatedAt = DateTimeOffset.UtcNow;

            return null;
        }

        var metadata = new FileMetadata
        {
            Id = Guid.NewGuid(),
            UserId = userId,
            Provider = "Google",
            ProviderFileId = file.Id,
            FileName = file.Name,
            MimeType = file.MimeType,
            FileSize = file.Size,
            ProviderModifiedAt = file.ModifiedTimeDateTimeOffset,
            WebViewLink = file.WebViewLink,
            ThumbnailLink = file.ThumbnailLink,
            ParentId = parentId,
            IsShared = file.Shared ?? false,
            Owners = owners,
            SyncedAt = DateTimeOffset.UtcNow,
            CreatedAt = DateTimeOffset.UtcNow,
            UpdatedAt = DateTimeOffset.UtcNow
        };

        _dbContext.FilesMetadata.Add(metadata);

        return metadata.Id;
    }

    /// <summary>
    /// Creates an authenticated Google Drive API service for the given user.
    /// Reads the encrypted OAuth token from the database, decrypts it,
    /// refreshes if expired, and returns a configured DriveService instance.
    /// </summary>
    private async Task<DriveService> CreateDriveServiceAsync(Guid userId)
    {
        var connection = await _dbContext.OAuthConnections
            .AsNoTracking()
            .FirstOrDefaultAsync(c => c.UserId == userId && c.Provider == OAuthProvider.Google)
            ?? throw new KeyNotFoundException(
                $"No Google OAuth connection found for user '{userId}'. " +
                "Please connect your Google account first.");

        var accessToken = TokenEncryption.Decrypt(connection.EncryptedAccessToken, _settings.TokenEncryptionKey);

        // Check if token is expired or about to expire (within 5 minutes)
        if (connection.AccessTokenExpiresAt.HasValue &&
            connection.AccessTokenExpiresAt.Value <= DateTimeOffset.UtcNow.AddMinutes(5))
        {
            _logger.LogDebug("Access token expired or expiring soon for user {UserId}. Refreshing.", userId);
            accessToken = await RefreshAccessTokenAsync(connection);
        }

        var credential = GoogleCredential.FromAccessToken(accessToken);

        return new DriveService(new BaseClientService.Initializer
        {
            HttpClientInitializer = credential,
            ApplicationName = _settings.ApplicationName
        });
    }

    /// <summary>
    /// Refreshes an expired Google OAuth access token using the refresh token.
    /// Updates the encrypted access token and expiration in the database.
    /// </summary>
    private async Task<string> RefreshAccessTokenAsync(OAuthConnection connection)
    {
        if (string.IsNullOrEmpty(connection.EncryptedRefreshToken))
        {
            throw new InvalidOperationException(
                $"Cannot refresh token for user '{connection.UserId}': no refresh token available. " +
                "The user must re-authenticate with Google.");
        }

        var refreshToken = TokenEncryption.Decrypt(connection.EncryptedRefreshToken, _settings.TokenEncryptionKey);

        var httpClient = _httpClientFactory.CreateClient("GoogleAuth");

        var tokenRequest = new FormUrlEncodedContent(new Dictionary<string, string>
        {
            ["client_id"] = _settings.ClientId,
            ["client_secret"] = _settings.ClientSecret,
            ["refresh_token"] = refreshToken,
            ["grant_type"] = "refresh_token"
        });

        var response = await httpClient.PostAsync(_settings.TokenEndpoint, tokenRequest);

        if (!response.IsSuccessStatusCode)
        {
            var errorBody = await response.Content.ReadAsStringAsync();
            _logger.LogError(
                "Failed to refresh Google token for user {UserId}. Status: {StatusCode}, Body: {Body}",
                connection.UserId, response.StatusCode, errorBody);

            throw new InvalidOperationException(
                $"Failed to refresh Google access token. Status: {response.StatusCode}. " +
                "The user may need to re-authenticate with Google.");
        }

        var tokenJson = await response.Content.ReadAsStringAsync();
        var tokenData = JsonSerializer.Deserialize<GoogleTokenResponse>(tokenJson);

        if (tokenData == null || string.IsNullOrEmpty(tokenData.AccessToken))
        {
            throw new InvalidOperationException("Google token refresh returned an invalid response.");
        }

        // Update the OAuth connection in the database with the new token
        var trackedConnection = await _dbContext.OAuthConnections
            .FirstAsync(c => c.Id == connection.Id);

        trackedConnection.EncryptedAccessToken = TokenEncryption.Encrypt(tokenData.AccessToken, _settings.TokenEncryptionKey);
        trackedConnection.AccessTokenExpiresAt = DateTimeOffset.UtcNow.AddSeconds(tokenData.ExpiresIn);
        trackedConnection.UpdatedAt = DateTimeOffset.UtcNow;

        // If Google returned a new refresh token, update it as well
        if (!string.IsNullOrEmpty(tokenData.RefreshToken))
        {
            trackedConnection.EncryptedRefreshToken = TokenEncryption.Encrypt(tokenData.RefreshToken, _settings.TokenEncryptionKey);
        }

        await _dbContext.SaveChangesAsync();

        _logger.LogInformation("Successfully refreshed Google access token for user {UserId}", connection.UserId);

        return tokenData.AccessToken;
    }

    /// <summary>
    /// Maps a Google Drive API File object to a platform GoogleFileDto.
    /// </summary>
    private static GoogleFileDto MapToDto(Google.Apis.Drive.v3.Data.File file)
    {
        return new GoogleFileDto
        {
            Id = file.Id,
            Name = file.Name,
            MimeType = file.MimeType,
            Size = file.Size,
            ModifiedTime = file.ModifiedTimeDateTimeOffset,
            Owners = file.Owners?.Select(o => o.DisplayName ?? o.EmailAddress ?? "Unknown").ToList() ?? [],
            Shared = file.Shared ?? false,
            WebViewLink = file.WebViewLink,
            ThumbnailLink = file.ThumbnailLink,
            ParentId = file.Parents?.FirstOrDefault()
        };
    }

    /// <summary>
    /// Publishes an audit event to the message bus for tracking file access operations.
    /// </summary>
    private async Task PublishAuditEventAsync(
        Guid userId,
        string action,
        string resourceType,
        string resourceId,
        IDictionary<string, string>? metadata = null)
    {
        try
        {
            await _publishEndpoint.Publish(new AuditEvent
            {
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
            // Audit publishing should not fail the primary operation
            _logger.LogWarning(ex, "Failed to publish audit event for action {Action} on {ResourceType}/{ResourceId}",
                action, resourceType, resourceId);
        }
    }

    /// <summary>
    /// Determines whether the given MIME type represents a Google Workspace file
    /// (Docs, Sheets, Slides, Drawings, etc.) that requires export rather than download.
    /// </summary>
    private static bool IsGoogleWorkspaceFile(string mimeType)
    {
        return mimeType.StartsWith("application/vnd.google-apps.", StringComparison.OrdinalIgnoreCase);
    }

    /// <summary>
    /// Returns the standard file extension for a given export MIME type.
    /// </summary>
    private static string GetExtensionForMimeType(string mimeType)
    {
        return mimeType.ToLowerInvariant() switch
        {
            "application/pdf" => ".pdf",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => ".docx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => ".xlsx",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation" => ".pptx",
            "text/plain" => ".txt",
            "text/html" => ".html",
            "text/csv" => ".csv",
            "application/rtf" => ".rtf",
            "application/zip" => ".zip",
            "image/png" => ".png",
            "image/jpeg" => ".jpg",
            "image/svg+xml" => ".svg",
            "application/epub+zip" => ".epub",
            _ => ".bin"
        };
    }
}

/// <summary>
/// Deserializes the Google OAuth token refresh response.
/// </summary>
internal class GoogleTokenResponse
{
    [System.Text.Json.Serialization.JsonPropertyName("access_token")]
    public string AccessToken { get; set; } = string.Empty;

    [System.Text.Json.Serialization.JsonPropertyName("expires_in")]
    public int ExpiresIn { get; set; }

    [System.Text.Json.Serialization.JsonPropertyName("refresh_token")]
    public string? RefreshToken { get; set; }

    [System.Text.Json.Serialization.JsonPropertyName("scope")]
    public string? Scope { get; set; }

    [System.Text.Json.Serialization.JsonPropertyName("token_type")]
    public string TokenType { get; set; } = string.Empty;
}
