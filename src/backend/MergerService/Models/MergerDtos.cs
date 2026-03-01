using System.Text.Json.Serialization;

namespace CenterPointInbox.MergerService.Models;

/// <summary>
/// Unified file representation returned to API consumers. Normalizes
/// differences between Google and Microsoft provider schemas.
/// </summary>
public class UnifiedFileDto
{
    [JsonPropertyName("id")]
    public Guid Id { get; init; }

    [JsonPropertyName("provider")]
    public string Provider { get; init; } = string.Empty;

    [JsonPropertyName("providerFileId")]
    public string ProviderFileId { get; init; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; init; } = string.Empty;

    [JsonPropertyName("mimeType")]
    public string MimeType { get; init; } = string.Empty;

    [JsonPropertyName("sizeBytes")]
    public long? SizeBytes { get; init; }

    [JsonPropertyName("modifiedAt")]
    public DateTimeOffset? ModifiedAt { get; init; }

    [JsonPropertyName("webUrl")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? WebUrl { get; init; }

    [JsonPropertyName("isShared")]
    public bool IsShared { get; init; }

    [JsonPropertyName("phiDetected")]
    public bool PhiDetected { get; init; }

    [JsonPropertyName("quarantined")]
    public bool Quarantined { get; init; }
}

/// <summary>
/// Paginated response wrapper for the unified file feed.
/// </summary>
public class UnifiedFileFeedResponse
{
    [JsonPropertyName("files")]
    public List<UnifiedFileDto> Files { get; init; } = [];

    [JsonPropertyName("totalCount")]
    public long TotalCount { get; init; }

    [JsonPropertyName("page")]
    public int Page { get; init; }

    [JsonPropertyName("pageSize")]
    public int PageSize { get; init; }

    [JsonPropertyName("hasMore")]
    public bool HasMore { get; init; }
}

/// <summary>
/// Request DTO for initiating a document format translation.
/// </summary>
public class TranslationRequestDto
{
    [JsonPropertyName("sourceFileId")]
    public Guid SourceFileId { get; init; }

    [JsonPropertyName("targetFormat")]
    public string TargetFormat { get; init; } = string.Empty;

    [JsonPropertyName("redactPhi")]
    public bool RedactPhi { get; init; }
}

/// <summary>
/// Response DTO representing a translation job and its current status.
/// </summary>
public class TranslationJobDto
{
    [JsonPropertyName("id")]
    public Guid Id { get; init; }

    [JsonPropertyName("status")]
    public string Status { get; init; } = string.Empty;

    [JsonPropertyName("sourceFormat")]
    public string SourceFormat { get; init; } = string.Empty;

    [JsonPropertyName("targetFormat")]
    public string TargetFormat { get; init; } = string.Empty;

    [JsonPropertyName("createdAt")]
    public DateTimeOffset CreatedAt { get; init; }

    [JsonPropertyName("completedAt")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public DateTimeOffset? CompletedAt { get; init; }

    [JsonPropertyName("error")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Error { get; init; }
}

/// <summary>
/// Query parameters for filtering, sorting, and paginating the unified file feed.
/// </summary>
public class FileFeedQuery
{
    /// <summary>
    /// Page number (1-based). Defaults to 1.
    /// </summary>
    public int Page { get; init; } = 1;

    /// <summary>
    /// Number of items per page. Defaults to 20, max 100.
    /// </summary>
    public int PageSize { get; init; } = 20;

    /// <summary>
    /// Optional provider filter: "google" or "microsoft".
    /// </summary>
    public string? Provider { get; init; }

    /// <summary>
    /// Optional search term matched against file names.
    /// </summary>
    public string? Search { get; init; }

    /// <summary>
    /// Sort field: "name", "modified", "size". Defaults to "modified".
    /// </summary>
    public string SortBy { get; init; } = "modified";

    /// <summary>
    /// Sort direction: "asc" or "desc". Defaults to "desc".
    /// </summary>
    public string SortDirection { get; init; } = "desc";
}
