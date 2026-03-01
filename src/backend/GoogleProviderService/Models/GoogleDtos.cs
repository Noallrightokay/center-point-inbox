using System.Text.Json.Serialization;

namespace CenterPointInbox.GoogleProviderService.Models;

/// <summary>
/// DTO representing a Google Drive file with metadata.
/// </summary>
public class GoogleFileDto
{
    [JsonPropertyName("id")]
    public string Id { get; init; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; init; } = string.Empty;

    [JsonPropertyName("mimeType")]
    public string MimeType { get; init; } = string.Empty;

    [JsonPropertyName("size")]
    public long? Size { get; init; }

    [JsonPropertyName("modifiedTime")]
    public DateTimeOffset? ModifiedTime { get; init; }

    [JsonPropertyName("owners")]
    public List<string> Owners { get; init; } = [];

    [JsonPropertyName("shared")]
    public bool Shared { get; init; }

    [JsonPropertyName("webViewLink")]
    public string? WebViewLink { get; init; }

    [JsonPropertyName("thumbnailLink")]
    public string? ThumbnailLink { get; init; }

    [JsonPropertyName("parentId")]
    public string? ParentId { get; init; }
}

/// <summary>
/// Response DTO for paginated file listing.
/// </summary>
public class GoogleFileListResponse
{
    [JsonPropertyName("files")]
    public List<GoogleFileDto> Files { get; init; } = [];

    [JsonPropertyName("nextPageToken")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? NextPageToken { get; init; }
}

/// <summary>
/// Response DTO for file content download or export.
/// </summary>
public class FileContentResponse
{
    public byte[] Content { get; init; } = [];

    public string MimeType { get; init; } = string.Empty;

    public string FileName { get; init; } = string.Empty;
}

/// <summary>
/// Request DTO for exporting a Google Workspace file to a specified format.
/// </summary>
public class ExportRequest
{
    [JsonPropertyName("fileId")]
    public string FileId { get; init; } = string.Empty;

    [JsonPropertyName("exportMimeType")]
    public string ExportMimeType { get; init; } = string.Empty;
}
