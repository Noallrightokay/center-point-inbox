using CenterPointInbox.AiAssistantService.Models;

namespace CenterPointInbox.AiAssistantService.Services;

/// <summary>
/// Service responsible for AI-powered question answering and document summarization.
/// Orchestrates semantic search, context building, and LLM chat completions while
/// enforcing ACL checks and prompt injection prevention.
/// </summary>
public interface IAiQueryService
{
    /// <summary>
    /// Answers a user's question by searching for relevant document chunks,
    /// building a context-augmented prompt, and calling the OpenAI chat completion API.
    /// Enforces ACL access checks and prompt injection prevention.
    /// </summary>
    /// <param name="userId">The ID of the user asking the question.</param>
    /// <param name="tenantId">The tenant scope for the query.</param>
    /// <param name="request">The query request containing the question and optional filters.</param>
    /// <returns>The AI-generated answer along with source references.</returns>
    Task<AiQueryResponse> AskAsync(Guid userId, Guid tenantId, AiQueryRequest request);

    /// <summary>
    /// Generates a summary and key points for a specific file's content.
    /// Retrieves all chunks for the file, builds a summarization prompt, and
    /// calls the OpenAI chat completion API. Enforces ACL access check.
    /// </summary>
    /// <param name="userId">The ID of the user requesting the summary.</param>
    /// <param name="tenantId">The tenant scope for the query.</param>
    /// <param name="request">The summarization request containing the target file ID.</param>
    /// <returns>The generated summary with extracted key points.</returns>
    Task<SummarizeResponse> SummarizeAsync(Guid userId, Guid tenantId, SummarizeRequest request);
}
