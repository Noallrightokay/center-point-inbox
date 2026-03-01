using System.Text.Json.Serialization;

namespace CenterPointInbox.MicrosoftProviderService.Models;

/// <summary>
/// DTO representing a Microsoft OneDrive/SharePoint file with metadata.
/// </summary>
public class MicrosoftFileDto
{
    [JsonPropertyName("id")]
    public string Id { get; init; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; init; } = string.Empty;

    [JsonPropertyName("mimeType")]
    public string MimeType { get; init; } = string.Empty;

    [JsonPropertyName("size")]
    public long? Size { get; init; }

    [JsonPropertyName("lastModifiedDateTime")]
    public DateTimeOffset? LastModifiedDateTime { get; init; }

    [JsonPropertyName("createdBy")]
    public string? CreatedBy { get; init; }

    [JsonPropertyName("webUrl")]
    public string? WebUrl { get; init; }

    [JsonPropertyName("parentPath")]
    public string? ParentPath { get; init; }

    [JsonPropertyName("shared")]
    public bool Shared { get; init; }
}

/// <summary>
/// Response DTO for paginated Microsoft file listing.
/// </summary>
public class MicrosoftFileListResponse
{
    [JsonPropertyName("files")]
    public List<MicrosoftFileDto> Files { get; init; } = [];

    [JsonPropertyName("nextLink")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? NextLink { get; init; }
}

/// <summary>
/// Response DTO for file content download.
/// </summary>
public class FileContentResponse
{
    public byte[] Content { get; init; } = [];

    public string MimeType { get; init; } = string.Empty;

    public string FileName { get; init; } = string.Empty;
}
