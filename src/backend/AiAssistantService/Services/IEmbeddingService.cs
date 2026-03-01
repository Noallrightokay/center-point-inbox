namespace CenterPointInbox.AiAssistantService.Services;

/// <summary>
/// Service responsible for generating and managing vector embeddings for document content.
/// Handles text chunking, OpenAI embedding API calls, and pgvector storage.
/// </summary>
public interface IEmbeddingService
{
    /// <summary>
    /// Generates embeddings for the given content by splitting it into chunks,
    /// calling the OpenAI embeddings API, and storing the results in the database.
    /// Existing embeddings for the file are deleted before inserting new ones.
    /// </summary>
    /// <param name="tenantId">The tenant that owns the file.</param>
    /// <param name="fileId">The file identifier.</param>
    /// <param name="content">The full text content to embed.</param>
    Task GenerateEmbeddingsAsync(Guid tenantId, Guid fileId, string content);

    /// <summary>
    /// Returns the number of embedding chunks stored for a given file.
    /// </summary>
    /// <param name="tenantId">The tenant that owns the file.</param>
    /// <param name="fileId">The file identifier.</param>
    /// <returns>The count of embedding chunks.</returns>
    Task<int> GetChunkCountAsync(Guid tenantId, Guid fileId);
}
