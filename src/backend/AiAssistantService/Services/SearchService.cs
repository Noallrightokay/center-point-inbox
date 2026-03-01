using System.Net.Http.Json;
using System.Text.Json.Serialization;
using CenterPointInbox.AiAssistantService.Configuration;
using CenterPointInbox.AiAssistantService.Models;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;
using Pgvector;
using Pgvector.EntityFrameworkCore;

namespace CenterPointInbox.AiAssistantService.Services;

/// <summary>
/// Implementation of ISearchService that performs semantic vector searches using pgvector
/// cosine distance. Generates an embedding for the query, then finds the nearest neighbors
/// in the embedding space, filtering by tenant and ACL permissions.
/// </summary>
public class SearchService : ISearchService
{
    private readonly AiDbContext _dbContext;
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly AiSettings _settings;
    private readonly ILogger<SearchService> _logger;

    public SearchService(
        AiDbContext dbContext,
        IHttpClientFactory httpClientFactory,
        IOptions<AiSettings> settings,
        ILogger<SearchService> logger)
    {
        _dbContext = dbContext;
        _httpClientFactory = httpClientFactory;
        _settings = settings.Value;
        _logger = logger;
    }

    /// <inheritdoc />
    public async Task<SearchResponse> SemanticSearchAsync(Guid userId, Guid tenantId, SearchRequest request)
    {
        _logger.LogInformation(
            "Semantic search requested by user {UserId} in tenant {TenantId}. Query length: {QueryLength}, MaxResults: {MaxResults}",
            userId, tenantId, request.Query.Length, request.MaxResults);

        // Step 1: Generate embedding vector for the search query
        var queryEmbedding = await GenerateQueryEmbeddingAsync(request.Query);
        var queryVector = new Vector(queryEmbedding);

        // Step 2: Get the set of file IDs the user is allowed to access (ACL check)
        var accessibleFileIds = await GetAccessibleFileIdsAsync(userId, tenantId);

        if (accessibleFileIds.Count == 0)
        {
            _logger.LogInformation(
                "User {UserId} has no accessible files in tenant {TenantId}. Returning empty results.",
                userId, tenantId);
            return new SearchResponse { Results = [] };
        }

        // Step 3: If specific file IDs are requested, intersect with accessible files
        var targetFileIds = accessibleFileIds;
        if (request.FileIds is { Length: > 0 })
        {
            targetFileIds = accessibleFileIds.Intersect(request.FileIds).ToHashSet();

            if (targetFileIds.Count == 0)
            {
                _logger.LogWarning(
                    "User {UserId} requested files they don't have access to. Returning empty results.",
                    userId);
                return new SearchResponse { Results = [] };
            }
        }

        // Step 4: Execute pgvector cosine distance search
        var maxResults = Math.Min(request.MaxResults, 50); // Cap at 50 results

        var results = await _dbContext.Embeddings
            .Where(e => e.TenantId == tenantId && targetFileIds.Contains(e.FileId))
            .OrderBy(e => e.EmbeddingVector.CosineDistance(queryVector))
            .Take(maxResults)
            .Select(e => new
            {
                e.FileId,
                e.ChunkText,
                e.ChunkIndex,
                Distance = e.EmbeddingVector.CosineDistance(queryVector)
            })
            .ToListAsync();

        _logger.LogInformation(
            "Semantic search returned {ResultCount} results for user {UserId}",
            results.Count, userId);

        // Step 5: Resolve file names from metadata
        var fileIds = results.Select(r => r.FileId).Distinct().ToList();
        var fileNameMap = await _dbContext.FilesMetadata
            .Where(f => fileIds.Contains(f.Id))
            .ToDictionaryAsync(f => f.Id, f => f.FileName);

        // Step 6: Map to response DTOs, converting cosine distance to similarity score
        var searchResults = results.Select(r => new SearchResult
        {
            FileId = r.FileId,
            FileName = fileNameMap.GetValueOrDefault(r.FileId, "Unknown"),
            ChunkText = r.ChunkText,
            Score = 1.0 - r.Distance, // Convert distance to similarity score (0..1)
            ChunkIndex = r.ChunkIndex
        }).ToList();

        return new SearchResponse { Results = searchResults };
    }

    /// <summary>
    /// Returns the set of file IDs that the user is allowed to access within their tenant.
    /// A user can access files they own or files that are shared within the tenant.
    /// Quarantined files are excluded.
    /// </summary>
    internal async Task<HashSet<Guid>> GetAccessibleFileIdsAsync(Guid userId, Guid tenantId)
    {
        var accessibleFiles = await _dbContext.FilesMetadata
            .Where(f => f.TenantId == tenantId
                        && !f.Quarantined
                        && (f.UserId == userId || f.IsShared))
            .Select(f => f.Id)
            .ToListAsync();

        return accessibleFiles.ToHashSet();
    }

    /// <summary>
    /// Generates an embedding vector for a single query string using the OpenAI embeddings API.
    /// </summary>
    internal async Task<float[]> GenerateQueryEmbeddingAsync(string query)
    {
        var httpClient = _httpClientFactory.CreateClient("OpenAI");

        var requestBody = new
        {
            model = _settings.EmbeddingModel,
            input = new[] { query }
        };

        var response = await httpClient.PostAsJsonAsync("v1/embeddings", requestBody);
        response.EnsureSuccessStatusCode();

        var responseContent = await response.Content.ReadFromJsonAsync<OpenAiEmbeddingResponse>();

        if (responseContent?.Data is null || responseContent.Data.Count == 0)
        {
            throw new InvalidOperationException(
                "OpenAI embeddings API returned an empty response for the query.");
        }

        return responseContent.Data[0].Embedding;
    }

    // ─────────────────────────────────────────────────────────────────────────
    // OpenAI Embeddings API response DTOs (shared shape with EmbeddingService)
    // ─────────────────────────────────────────────────────────────────────────

    internal sealed class OpenAiEmbeddingResponse
    {
        [JsonPropertyName("data")]
        public List<OpenAiEmbeddingData> Data { get; set; } = [];
    }

    internal sealed class OpenAiEmbeddingData
    {
        [JsonPropertyName("index")]
        public int Index { get; set; }

        [JsonPropertyName("embedding")]
        public float[] Embedding { get; set; } = [];
    }
}
