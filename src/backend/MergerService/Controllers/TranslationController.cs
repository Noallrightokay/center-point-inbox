using CenterPointInbox.MergerService.Models;
using CenterPointInbox.MergerService.Services;
using CenterPointInbox.Shared.Middleware;
using CenterPointInbox.Shared.Models;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;

namespace CenterPointInbox.MergerService.Controllers;

/// <summary>
/// API controller for document format translation operations.
/// Allows users to request file format conversions (e.g., DOCX to PDF)
/// and track the status of translation jobs.
/// </summary>
[ApiController]
[Route("api/translate")]
[Authorize]
public class TranslationController : ControllerBase
{
    private readonly ITranslationService _translationService;
    private readonly ILogger<TranslationController> _logger;

    public TranslationController(ITranslationService translationService, ILogger<TranslationController> logger)
    {
        _translationService = translationService;
        _logger = logger;
    }

    /// <summary>
    /// Requests a new document format translation. Creates a translation job and
    /// publishes a TranslationRequested event for asynchronous processing.
    /// </summary>
    [HttpPost]
    [ProducesResponseType(typeof(ApiResponse<TranslationJobDto>), StatusCodes.Status201Created)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status400BadRequest)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> RequestTranslation([FromBody] TranslationRequestDto request)
    {
        var userContext = UserContext.FromClaimsPrincipal(User);
        var correlationId = HttpContext.GetCorrelationId();

        _logger.LogInformation(
            "Translation requested for file {SourceFileId} -> {TargetFormat} by user {UserId} [CorrelationId: {CorrelationId}]",
            request.SourceFileId, request.TargetFormat, userContext.UserId, correlationId);

        var result = await _translationService.RequestTranslationAsync(
            userContext.UserId, userContext.TenantId, request);

        return StatusCode(StatusCodes.Status201Created,
            ApiResponse<TranslationJobDto>.Success(result, correlationId));
    }

    /// <summary>
    /// Retrieves the current status of a specific translation job.
    /// </summary>
    [HttpGet("{jobId:guid}")]
    [ProducesResponseType(typeof(ApiResponse<TranslationJobDto>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status404NotFound)]
    public async Task<IActionResult> GetTranslationStatus([FromRoute] Guid jobId)
    {
        var userContext = UserContext.FromClaimsPrincipal(User);
        var correlationId = HttpContext.GetCorrelationId();

        _logger.LogInformation(
            "Translation status requested for job {JobId} by user {UserId} [CorrelationId: {CorrelationId}]",
            jobId, userContext.UserId, correlationId);

        var result = await _translationService.GetTranslationStatusAsync(
            userContext.UserId, userContext.TenantId, jobId);

        return Ok(ApiResponse<TranslationJobDto>.Success(result, correlationId));
    }

    /// <summary>
    /// Lists all translation jobs for the authenticated user, ordered by
    /// creation date descending.
    /// </summary>
    [HttpGet]
    [ProducesResponseType(typeof(ApiResponse<List<TranslationJobDto>>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<object>), StatusCodes.Status401Unauthorized)]
    public async Task<IActionResult> GetUserTranslations()
    {
        var userContext = UserContext.FromClaimsPrincipal(User);
        var correlationId = HttpContext.GetCorrelationId();

        _logger.LogInformation(
            "Translation list requested by user {UserId} [CorrelationId: {CorrelationId}]",
            userContext.UserId, correlationId);

        var result = await _translationService.GetUserTranslationsAsync(
            userContext.UserId, userContext.TenantId);

        return Ok(ApiResponse<List<TranslationJobDto>>.Success(result, correlationId));
    }
}
