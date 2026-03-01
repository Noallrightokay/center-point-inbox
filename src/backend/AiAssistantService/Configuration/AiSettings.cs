namespace CenterPointInbox.AiAssistantService.Configuration;

/// <summary>
/// Configuration POCO for AI Assistant settings.
/// Bound from the "AiSettings" section of appsettings.json.
/// </summary>
public class AiSettings
{
    public const string SectionName = "AiSettings";

    /// <summary>
    /// OpenAI API key for authentication.
    /// </summary>
    public string OpenAiApiKey { get; set; } = string.Empty;

    /// <summary>
    /// Chat completion model identifier. Defaults to "gpt-4o".
    /// </summary>
    public string OpenAiModel { get; set; } = "gpt-4o";

    /// <summary>
    /// Embedding model identifier. Defaults to "text-embedding-3-small".
    /// </summary>
    public string EmbeddingModel { get; set; } = "text-embedding-3-small";

    /// <summary>
    /// Maximum number of tokens per text chunk when splitting documents.
    /// </summary>
    public int MaxChunkTokens { get; set; } = 500;

    /// <summary>
    /// Number of overlapping tokens between consecutive chunks to preserve context continuity.
    /// </summary>
    public int ChunkOverlapTokens { get; set; } = 50;

    /// <summary>
    /// Maximum number of context chunks to include in an AI query prompt.
    /// </summary>
    public int MaxContextChunks { get; set; } = 10;

    /// <summary>
    /// Maximum allowed character length for user queries to prevent abuse.
    /// </summary>
    public int MaxQueryLength { get; set; } = 2000;
}
