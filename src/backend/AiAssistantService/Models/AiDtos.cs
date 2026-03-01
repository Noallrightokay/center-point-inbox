using System.Text.Json.Serialization;

namespace CenterPointInbox.AiAssistantService.Models;

// ─────────────────────────────────────────────────────────────────────────────
// AI Query (Ask) DTOs
// ─────────────────────────────────────────────────────────────────────────────

/// <summary>
/// Request DTO for asking a question against indexed document content.
/// </summary>
public class AiQueryRequest
{
    [JsonPropertyName("query")]
    public string Query { get; init; } = string.Empty;

    /// <summary>
    /// Optional list of file IDs to restrict the search scope.
    /// When null or empty, all accessible files are searched.
    /// </summary>
    [JsonPropertyName("fileIds")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public Guid[]? FileIds { get; init; }

    [JsonPropertyName("maxResults")]
    public int MaxResults { get; init; } = 5;
}

/// <summary>
/// Response DTO containing the AI-generated answer and supporting source references.
/// </summary>
public class AiQueryResponse
{
    [JsonPropertyName("answer")]
    public string Answer { get; init; } = string.Empty;

    [JsonPropertyName("sources")]
    public List<SourceReference> Sources { get; init; } = [];
}

/// <summary>
/// A reference to a specific document chunk that supported the AI's answer.
/// </summary>
public class SourceReference
{
    [JsonPropertyName("fileId")]
    public Guid FileId { get; init; }

    [JsonPropertyName("fileName")]
    public string FileName { get; init; } = string.Empty;

    [JsonPropertyName("chunkText")]
    public string ChunkText { get; init; } = string.Empty;

    [JsonPropertyName("score")]
    public double Score { get; init; }
}

// ─────────────────────────────────────────────────────────────────────────────
// Summarize DTOs
// ─────────────────────────────────────────────────────────────────────────────

/// <summary>
/// Request DTO for generating a summary of a specific file's content.
/// </summary>
public class SummarizeRequest
{
    [JsonPropertyName("fileId")]
    public Guid FileId { get; init; }
}

/// <summary>
/// Response DTO containing the generated summary and extracted key points.
/// </summary>
public class SummarizeResponse
{
    [JsonPropertyName("summary")]
    public string Summary { get; init; } = string.Empty;

    [JsonPropertyName("keyPoints")]
    public List<string> KeyPoints { get; init; } = [];

    [JsonPropertyName("fileId")]
    public Guid FileId { get; init; }

    [JsonPropertyName("fileName")]
    public string FileName { get; init; } = string.Empty;
}

// ─────────────────────────────────────────────────────────────────────────────
// Semantic Search DTOs
// ─────────────────────────────────────────────────────────────────────────────

/// <summary>
/// Request DTO for performing a semantic vector search across indexed documents.
/// </summary>
public class SearchRequest
{
    [JsonPropertyName("query")]
    public string Query { get; init; } = string.Empty;

    [JsonPropertyName("maxResults")]
    public int MaxResults { get; init; } = 10;

    /// <summary>
    /// Optional list of file IDs to restrict the search scope.
    /// When null or empty, all accessible files are searched.
    /// </summary>
    [JsonPropertyName("fileIds")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public Guid[]? FileIds { get; init; }
}

/// <summary>
/// Response DTO containing semantically similar document chunks.
/// </summary>
public class SearchResponse
{
    [JsonPropertyName("results")]
    public List<SearchResult> Results { get; init; } = [];
}

/// <summary>
/// A single search result representing a matching document chunk.
/// </summary>
public class SearchResult
{
    [JsonPropertyName("fileId")]
    public Guid FileId { get; init; }

    [JsonPropertyName("fileName")]
    public string FileName { get; init; } = string.Empty;

    [JsonPropertyName("chunkText")]
    public string ChunkText { get; init; } = string.Empty;

    [JsonPropertyName("score")]
    public double Score { get; init; }

    [JsonPropertyName("chunkIndex")]
    public int ChunkIndex { get; init; }
}

// ─────────────────────────────────────────────────────────────────────────────
// Embedding Status DTO
// ─────────────────────────────────────────────────────────────────────────────

/// <summary>
/// Response DTO showing the indexing status of a file's embeddings.
/// </summary>
public class EmbeddingStatusResponse
{
    [JsonPropertyName("fileId")]
    public Guid FileId { get; init; }

    [JsonPropertyName("chunkCount")]
    public int ChunkCount { get; init; }

    [JsonPropertyName("lastIndexedAt")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public DateTimeOffset? LastIndexedAt { get; init; }
}
