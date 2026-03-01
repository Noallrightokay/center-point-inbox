using System.Net.Http.Json;
using System.Text.Json;
using System.Text.Json.Serialization;
using CenterPointInbox.AiAssistantService.Configuration;
using CenterPointInbox.AiAssistantService.Models;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;
using Pgvector;

namespace CenterPointInbox.AiAssistantService.Services;

/// <summary>
/// Implementation of IEmbeddingService that uses OpenAI's text-embedding-3-small model
/// to generate vector embeddings for document chunks and stores them in PostgreSQL via pgvector.
/// </summary>
public class EmbeddingService : IEmbeddingService
{
    private readonly AiDbContext _dbContext;
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly AiSettings _settings;
    private readonly ILogger<EmbeddingService> _logger;

    public EmbeddingService(
        AiDbContext dbContext,
        IHttpClientFactory httpClientFactory,
        IOptions<AiSettings> settings,
        ILogger<EmbeddingService> logger)
    {
        _dbContext = dbContext;
        _httpClientFactory = httpClientFactory;
        _settings = settings.Value;
        _logger = logger;
    }

    /// <inheritdoc />
    public async Task GenerateEmbeddingsAsync(Guid tenantId, Guid fileId, string content)
    {
        if (string.IsNullOrWhiteSpace(content))
        {
            _logger.LogWarning(
                "Empty content provided for file {FileId} in tenant {TenantId}. Skipping embedding generation.",
                fileId, tenantId);
            return;
        }

        _logger.LogInformation(
            "Generating embeddings for file {FileId} in tenant {TenantId}. Content length: {ContentLength} chars",
            fileId, tenantId, content.Length);

        // Step 1: Split content into overlapping chunks
        var chunks = SplitIntoChunks(content, _settings.MaxChunkTokens, _settings.ChunkOverlapTokens);

        _logger.LogInformation(
            "File {FileId}: Split into {ChunkCount} chunks (max {MaxTokens} tokens, {OverlapTokens} overlap)",
            fileId, chunks.Count, _settings.MaxChunkTokens, _settings.ChunkOverlapTokens);

        // Step 2: Generate embeddings for all chunks via OpenAI API
        var embeddings = await GenerateEmbeddingVectorsAsync(chunks);

        _logger.LogInformation(
            "File {FileId}: Generated {EmbeddingCount} embedding vectors",
            fileId, embeddings.Count);

        // Step 3: Delete existing embeddings for this file (re-indexing scenario)
        var deletedCount = await _dbContext.Embeddings
            .Where(e => e.FileId == fileId && e.TenantId == tenantId)
            .ExecuteDeleteAsync();

        if (deletedCount > 0)
        {
            _logger.LogInformation(
                "File {FileId}: Deleted {DeletedCount} existing embedding(s) before re-indexing",
                fileId, deletedCount);
        }

        // Step 4: Insert new embeddings
        var embeddingEntities = new List<Embedding>();
        for (var i = 0; i < chunks.Count; i++)
        {
            embeddingEntities.Add(new Embedding
            {
                Id = Guid.NewGuid(),
                TenantId = tenantId,
                FileId = fileId,
                ChunkIndex = i,
                ChunkText = chunks[i],
                EmbeddingVector = new Vector(embeddings[i]),
                Metadata = JsonSerializer.Serialize(new { chunkIndex = i, totalChunks = chunks.Count }),
                CreatedAt = DateTimeOffset.UtcNow
            });
        }

        _dbContext.Embeddings.AddRange(embeddingEntities);
        await _dbContext.SaveChangesAsync();

        _logger.LogInformation(
            "File {FileId}: Successfully stored {Count} embeddings in database",
            fileId, embeddingEntities.Count);
    }

    /// <inheritdoc />
    public async Task<int> GetChunkCountAsync(Guid tenantId, Guid fileId)
    {
        return await _dbContext.Embeddings
            .CountAsync(e => e.FileId == fileId && e.TenantId == tenantId);
    }

    /// <summary>
    /// Splits text content into overlapping chunks based on approximate token count.
    /// Uses a simple word-based approximation where 1 token ~ 4 characters.
    /// </summary>
    internal static List<string> SplitIntoChunks(string content, int maxChunkTokens, int overlapTokens)
    {
        var chunks = new List<string>();

        // Approximate characters per token (English text averages ~4 chars per token)
        const int charsPerToken = 4;
        var maxChunkChars = maxChunkTokens * charsPerToken;
        var overlapChars = overlapTokens * charsPerToken;

        if (content.Length <= maxChunkChars)
        {
            chunks.Add(content.Trim());
            return chunks;
        }

        var position = 0;
        while (position < content.Length)
        {
            var end = Math.Min(position + maxChunkChars, content.Length);

            // Try to break at a sentence or word boundary
            if (end < content.Length)
            {
                var lastPeriod = content.LastIndexOf('.', end, Math.Min(end - position, 200));
                var lastNewline = content.LastIndexOf('\n', end, Math.Min(end - position, 200));
                var lastSpace = content.LastIndexOf(' ', end, Math.Min(end - position, 100));

                // Prefer sentence boundary, then newline, then word boundary
                if (lastPeriod > position + maxChunkChars / 2)
                {
                    end = lastPeriod + 1;
                }
                else if (lastNewline > position + maxChunkChars / 2)
                {
                    end = lastNewline + 1;
                }
                else if (lastSpace > position + maxChunkChars / 2)
                {
                    end = lastSpace + 1;
                }
            }

            var chunk = content[position..end].Trim();
            if (!string.IsNullOrWhiteSpace(chunk))
            {
                chunks.Add(chunk);
            }

            // Move position forward with overlap
            position = end - overlapChars;

            // Ensure forward progress
            if (position <= (end - maxChunkChars + overlapChars))
            {
                position = end;
            }
        }

        return chunks;
    }

    /// <summary>
    /// Calls the OpenAI embeddings API to generate vector representations for the given text chunks.
    /// Sends chunks in batches to respect API limits.
    /// </summary>
    internal async Task<List<float[]>> GenerateEmbeddingVectorsAsync(List<string> chunks)
    {
        var httpClient = _httpClientFactory.CreateClient("OpenAI");
        var allEmbeddings = new List<float[]>();

        // Process in batches of up to 100 chunks (OpenAI batch limit)
        const int batchSize = 100;
        for (var batchStart = 0; batchStart < chunks.Count; batchStart += batchSize)
        {
            var batch = chunks.Skip(batchStart).Take(batchSize).ToList();

            var requestBody = new
            {
                model = _settings.EmbeddingModel,
                input = batch
            };

            var response = await httpClient.PostAsJsonAsync("v1/embeddings", requestBody);
            response.EnsureSuccessStatusCode();

            var responseContent = await response.Content.ReadFromJsonAsync<OpenAiEmbeddingResponse>();

            if (responseContent?.Data is null || responseContent.Data.Count == 0)
            {
                throw new InvalidOperationException(
                    "OpenAI embeddings API returned an empty response.");
            }

            // Sort by index to maintain order
            var sortedData = responseContent.Data.OrderBy(d => d.Index).ToList();

            foreach (var item in sortedData)
            {
                allEmbeddings.Add(item.Embedding);
            }

            _logger.LogDebug(
                "Processed embedding batch {BatchStart}-{BatchEnd} of {Total}",
                batchStart, batchStart + batch.Count - 1, chunks.Count);
        }

        return allEmbeddings;
    }

    // ─────────────────────────────────────────────────────────────────────────
    // OpenAI Embeddings API response DTOs
    // ─────────────────────────────────────────────────────────────────────────

    internal sealed class OpenAiEmbeddingResponse
    {
        [JsonPropertyName("data")]
        public List<OpenAiEmbeddingData> Data { get; set; } = [];

        [JsonPropertyName("model")]
        public string Model { get; set; } = string.Empty;

        [JsonPropertyName("usage")]
        public OpenAiUsage? Usage { get; set; }
    }

    internal sealed class OpenAiEmbeddingData
    {
        [JsonPropertyName("index")]
        public int Index { get; set; }

        [JsonPropertyName("embedding")]
        public float[] Embedding { get; set; } = [];
    }

    internal sealed class OpenAiUsage
    {
        [JsonPropertyName("prompt_tokens")]
        public int PromptTokens { get; set; }

        [JsonPropertyName("total_tokens")]
        public int TotalTokens { get; set; }
    }
}
