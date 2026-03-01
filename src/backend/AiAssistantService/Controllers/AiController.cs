using CenterPointInbox.AiAssistantService.Models;
using CenterPointInbox.AiAssistantService.Services;
using CenterPointInbox.Shared.Middleware;
using CenterPointInbox.Shared.Models;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

namespace CenterPointInbox.AiAssistantService.Controllers;

/// <summary>
/// API controller for AI-powered document operations including question answering,
/// summarization, semantic search, and embedding status queries.
/// All endpoints require authentication and enforce tenant-scoped ACL checks.
/// </summary>
[ApiController]
[Route("api/ai")]
[Authorize]
public class AiController : ControllerBase
{
    private readonly IAiQueryService _aiQueryService;
    private readonly ISearchService _searchService;
    private readonly IEmbeddingService _embeddingService;
    private readonly ILogger<AiController> _logger;

    public AiController(
        IAiQueryService aiQueryService,
        ISearchService searchService,
        IEmbeddingService embeddingService,
        ILogger<AiController> logger)
    {
        _aiQueryService = aiQueryService;
        _searchService = searchService;
        _embeddingService = embeddingService;
        _logger = logger;
    }

    /// <summary>
    /// Asks a question against indexed document content. Performs semantic search for
    /// relevant context, then generates an AI answer with source citations.
    /// </summary>
    [HttpPost("ask")]
    [ProducesResponseType(typeof(ApiResponse<AiQueryResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status400BadRequest)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    public async Task<IActionResult> Ask([FromBody] AiQueryRequest request)
    {
        var userContext = UserContext.FromClaimsPrincipal(User);
        var correlationId = HttpContext.GetCorrelationId();

        _logger.LogInformation(
            "AI ask request received from user {UserId} in tenant {TenantId} [CorrelationId: {CorrelationId}]",
            userContext.UserId, userContext.TenantId, correlationId);

        var result = await _aiQueryService.AskAsync(
            userContext.UserId, userContext.TenantId, request);

        return Ok(ApiResponse<AiQueryResponse>.Success(result, correlationId));
    }

    /// <summary>
    /// Generates a summary and key points for a specific file's indexed content.
    /// </summary>
    [HttpPost("summarize")]
    [ProducesResponseType(typeof(ApiResponse<SummarizeResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status400BadRequest)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> Summarize([FromBody] SummarizeRequest request)
    {
        var userContext = UserContext.FromClaimsPrincipal(User);
        var correlationId = HttpContext.GetCorrelationId();

        _logger.LogInformation(
            "Summarization request received for file {FileId} from user {UserId} [CorrelationId: {CorrelationId}]",
            request.FileId, userContext.UserId, correlationId);

        var result = await _aiQueryService.SummarizeAsync(
            userContext.UserId, userContext.TenantId, request);

        return Ok(ApiResponse<SummarizeResponse>.Success(result, correlationId));
    }

    /// <summary>
    /// Performs a semantic search across indexed document embeddings.
    /// Returns ranked results with similarity scores and chunk text.
    /// </summary>
    [HttpPost("search")]
    [ProducesResponseType(typeof(ApiResponse<SearchResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status400BadRequest)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    public async Task<IActionResult> Search([FromBody] SearchRequest request)
    {
        var userContext = UserContext.FromClaimsPrincipal(User);
        var correlationId = HttpContext.GetCorrelationId();

        _logger.LogInformation(
            "Semantic search request received from user {UserId} in tenant {TenantId} [CorrelationId: {CorrelationId}]",
            userContext.UserId, userContext.TenantId, correlationId);

        var result = await _searchService.SemanticSearchAsync(
            userContext.UserId, userContext.TenantId, request);

        return Ok(ApiResponse<SearchResponse>.Success(result, correlationId));
    }

    /// <summary>
    /// Returns the embedding/indexing status for a specific file, including chunk count
    /// and the timestamp of the most recent indexing operation.
    /// </summary>
    [HttpGet("status/{fileId:guid}")]
    [ProducesResponseType(typeof(ApiResponse<EmbeddingStatusResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    public async Task<IActionResult> GetEmbeddingStatus([FromRoute] Guid fileId)
    {
        var userContext = UserContext.FromClaimsPrincipal(User);
        var correlationId = HttpContext.GetCorrelationId();

        _logger.LogInformation(
            "Embedding status request for file {FileId} from user {UserId} [CorrelationId: {CorrelationId}]",
            fileId, userContext.UserId, correlationId);

        var chunkCount = await _embeddingService.GetChunkCountAsync(
            userContext.TenantId, fileId);

        // Get the most recent embedding creation timestamp for this file
        DateTimeOffset? lastIndexedAt = null;
        if (chunkCount > 0)
        {
            lastIndexedAt = await GetLastIndexedAtAsync(userContext.TenantId, fileId);
        }

        var result = new EmbeddingStatusResponse
        {
            FileId = fileId,
            ChunkCount = chunkCount,
            LastIndexedAt = lastIndexedAt
        };

        return Ok(ApiResponse<EmbeddingStatusResponse>.Success(result, correlationId));
    }

    /// <summary>
    /// Retrieves the most recent embedding creation timestamp for a file.
    /// </summary>
    private async Task<DateTimeOffset?> GetLastIndexedAtAsync(Guid tenantId, Guid fileId)
    {
        // Use the embedding service's DbContext through controller-level DI
        // This is a simple query, so direct access via a helper is acceptable
        var dbContext = HttpContext.RequestServices.GetRequiredService<Models.AiDbContext>();

        return await dbContext.Embeddings
            .Where(e => e.FileId == fileId && e.TenantId == tenantId)
            .MaxAsync(e => (DateTimeOffset?)e.CreatedAt);
    }
}
