using CenterPointInbox.AiAssistantService.Models;

namespace CenterPointInbox.AiAssistantService.Services;

/// <summary>
/// Service responsible for performing semantic vector searches against indexed document embeddings.
/// Enforces tenant isolation and ACL checks before returning results.
/// </summary>
public interface ISearchService
{
    /// <summary>
    /// Performs a semantic search using pgvector cosine distance, filtered by tenant
    /// and optionally by specific file IDs. Results are ACL-checked to ensure the
    /// requesting user has access to each returned file.
    /// </summary>
    /// <param name="userId">The ID of the user performing the search.</param>
    /// <param name="tenantId">The tenant to scope the search within.</param>
    /// <param name="request">The search request containing query text and filters.</param>
    /// <returns>A response containing ranked search results with similarity scores.</returns>
    Task<SearchResponse> SemanticSearchAsync(Guid userId, Guid tenantId, SearchRequest request);
}
