using Pgvector;

namespace CenterPointInbox.AiAssistantService.Models;

/// <summary>
/// Entity mapping to the embeddings table. Stores a vector embedding for
/// a single text chunk extracted from a file, enabling semantic search via pgvector.
/// </summary>
public class Embedding
{
    public Guid Id { get; set; }
    public Guid TenantId { get; set; }
    public Guid FileId { get; set; }
    public int ChunkIndex { get; set; }
    public string ChunkText { get; set; } = string.Empty;

    /// <summary>
    /// The 1536-dimensional embedding vector produced by text-embedding-3-small.
    /// Stored as a pgvector vector(1536) column.
    /// </summary>
    public Vector EmbeddingVector { get; set; } = null!;

    /// <summary>
    /// Optional JSON metadata associated with this chunk (e.g., page number, section heading).
    /// Stored as a JSONB column.
    /// </summary>
    public string? Metadata { get; set; }

    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
}
