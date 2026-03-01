using CenterPointInbox.MergerService.Models;

namespace CenterPointInbox.MergerService.Services;

/// <summary>
/// Service interface for document format translation operations.
/// Manages translation job lifecycle: creation, status tracking, and listing.
/// Translation work is performed asynchronously by the TranslationWorker service.
/// </summary>
public interface ITranslationService
{
    /// <summary>
    /// Creates a new translation job for a source file. Persists the job record
    /// in the database and publishes a TranslationRequested event via MassTransit
    /// for the TranslationWorker to pick up.
    /// </summary>
    /// <param name="userId">The authenticated user's ID.</param>
    /// <param name="tenantId">The user's tenant ID for multi-tenant isolation.</param>
    /// <param name="request">The translation request containing source file ID, target format, and PHI redaction flag.</param>
    /// <returns>The created translation job DTO.</returns>
    Task<TranslationJobDto> RequestTranslationAsync(Guid userId, Guid tenantId, TranslationRequestDto request);

    /// <summary>
    /// Retrieves the current status of a translation job.
    /// </summary>
    /// <param name="userId">The authenticated user's ID.</param>
    /// <param name="tenantId">The user's tenant ID for multi-tenant isolation.</param>
    /// <param name="jobId">The translation job ID.</param>
    /// <returns>The translation job DTO with current status.</returns>
    Task<TranslationJobDto> GetTranslationStatusAsync(Guid userId, Guid tenantId, Guid jobId);

    /// <summary>
    /// Lists all translation jobs for the authenticated user within their tenant.
    /// Ordered by creation date descending (most recent first).
    /// </summary>
    /// <param name="userId">The authenticated user's ID.</param>
    /// <param name="tenantId">The user's tenant ID for multi-tenant isolation.</param>
    /// <returns>A list of translation job DTOs.</returns>
    Task<List<TranslationJobDto>> GetUserTranslationsAsync(Guid userId, Guid tenantId);
}
