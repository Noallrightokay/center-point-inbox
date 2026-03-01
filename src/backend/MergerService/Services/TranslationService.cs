using CenterPointInbox.MergerService.Models;
using CenterPointInbox.Shared.Contracts;
using MassTransit;
using Microsoft.EntityFrameworkCore;

namespace CenterPointInbox.MergerService.Services;

/// <summary>
/// Implementation of ITranslationService. Manages the lifecycle of document format
/// translation jobs: creation, status tracking, and listing. Actual translation work
/// is performed asynchronously by the TranslationWorker service via MassTransit messaging.
/// </summary>
public class TranslationService : ITranslationService
{
    private readonly MergerDbContext _dbContext;
    private readonly IPublishEndpoint _publishEndpoint;
    private readonly ILogger<TranslationService> _logger;

    public TranslationService(
        MergerDbContext dbContext,
        IPublishEndpoint publishEndpoint,
        ILogger<TranslationService> logger)
    {
        _dbContext = dbContext;
        _publishEndpoint = publishEndpoint;
        _logger = logger;
    }

    /// <inheritdoc />
    public async Task<TranslationJobDto> RequestTranslationAsync(
        Guid userId, Guid tenantId, TranslationRequestDto request)
    {
        // Validate that the source file exists and belongs to the user
        var sourceFile = await _dbContext.FilesMetadata
            .AsNoTracking()
            .FirstOrDefaultAsync(f =>
                f.Id == request.SourceFileId &&
                f.UserId == userId &&
                f.TenantId == tenantId)
            ?? throw new KeyNotFoundException(
                $"Source file '{request.SourceFileId}' not found.");

        if (sourceFile.Quarantined)
        {
            throw new InvalidOperationException(
                $"File '{sourceFile.FileName}' is quarantined and cannot be translated.");
        }

        // Derive source format from the file's MIME type
        var sourceFormat = DeriveFormatFromMimeType(sourceFile.MimeType);

        if (string.IsNullOrWhiteSpace(request.TargetFormat))
        {
            throw new ArgumentException("Target format is required.", nameof(request));
        }

        // Create the translation job record
        var job = new TranslationJob
        {
            Id = Guid.NewGuid(),
            UserId = userId,
            TenantId = tenantId,
            SourceFileId = request.SourceFileId,
            SourceFormat = sourceFormat,
            TargetFormat = request.TargetFormat.Trim().ToLowerInvariant(),
            Status = "queued",
            PhiRedacted = request.RedactPhi,
            CreatedAt = DateTimeOffset.UtcNow,
            UpdatedAt = DateTimeOffset.UtcNow
        };

        _dbContext.TranslationJobs.Add(job);
        await _dbContext.SaveChangesAsync();

        _logger.LogInformation(
            "Translation job {JobId} created for file {SourceFileId} ({SourceFormat} -> {TargetFormat}) by user {UserId}",
            job.Id, request.SourceFileId, sourceFormat, request.TargetFormat, userId);

        // Publish TranslationRequested event for the TranslationWorker to consume
        await _publishEndpoint.Publish(new TranslationRequested
        {
            TenantId = tenantId,
            UserId = userId,
            SourceFileId = request.SourceFileId,
            SourceFormat = sourceFormat,
            TargetFormat = job.TargetFormat,
            RedactPhi = request.RedactPhi,
            RequestedAt = DateTimeOffset.UtcNow
        });

        _logger.LogInformation(
            "TranslationRequested event published for job {JobId}",
            job.Id);

        // Publish audit event
        await PublishAuditEventAsync(userId, tenantId, "RequestTranslation", "TranslationJob", job.Id.ToString(),
            new Dictionary<string, string>
            {
                ["sourceFileId"] = request.SourceFileId.ToString(),
                ["sourceFormat"] = sourceFormat,
                ["targetFormat"] = job.TargetFormat,
                ["redactPhi"] = request.RedactPhi.ToString()
            });

        return MapToDto(job);
    }

    /// <inheritdoc />
    public async Task<TranslationJobDto> GetTranslationStatusAsync(Guid userId, Guid tenantId, Guid jobId)
    {
        var job = await _dbContext.TranslationJobs
            .AsNoTracking()
            .FirstOrDefaultAsync(j =>
                j.Id == jobId &&
                j.UserId == userId &&
                j.TenantId == tenantId)
            ?? throw new KeyNotFoundException($"Translation job '{jobId}' not found.");

        return MapToDto(job);
    }

    /// <inheritdoc />
    public async Task<List<TranslationJobDto>> GetUserTranslationsAsync(Guid userId, Guid tenantId)
    {
        var jobs = await _dbContext.TranslationJobs
            .AsNoTracking()
            .Where(j => j.UserId == userId && j.TenantId == tenantId)
            .OrderByDescending(j => j.CreatedAt)
            .ToListAsync();

        return jobs.Select(MapToDto).ToList();
    }

    /// <summary>
    /// Derives a short format identifier from a MIME type string.
    /// </summary>
    private static string DeriveFormatFromMimeType(string mimeType)
    {
        return mimeType.ToLowerInvariant() switch
        {
            "application/pdf" => "pdf",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
            "application/msword" => "doc",
            "application/vnd.ms-excel" => "xls",
            "application/vnd.ms-powerpoint" => "ppt",
            "text/plain" => "txt",
            "text/csv" => "csv",
            "text/html" => "html",
            "application/rtf" => "rtf",
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "application/vnd.google-apps.document" => "gdoc",
            "application/vnd.google-apps.spreadsheet" => "gsheet",
            "application/vnd.google-apps.presentation" => "gslide",
            _ => mimeType.Contains('/') ? mimeType.Split('/').Last() : mimeType
        };
    }

    /// <summary>
    /// Maps a TranslationJob entity to a TranslationJobDto.
    /// </summary>
    private static TranslationJobDto MapToDto(TranslationJob job)
    {
        return new TranslationJobDto
        {
            Id = job.Id,
            Status = job.Status,
            SourceFormat = job.SourceFormat,
            TargetFormat = job.TargetFormat,
            CreatedAt = job.CreatedAt,
            CompletedAt = job.CompletedAt,
            Error = job.ErrorMessage
        };
    }

    /// <summary>
    /// Publishes an audit event to the message bus. Failures are logged but do not
    /// interrupt the primary operation.
    /// </summary>
    private async Task PublishAuditEventAsync(
        Guid userId,
        Guid tenantId,
        string action,
        string resourceType,
        string resourceId,
        IDictionary<string, string>? metadata = null)
    {
        try
        {
            await _publishEndpoint.Publish(new AuditEvent
            {
                TenantId = tenantId,
                UserId = userId,
                Action = action,
                ResourceType = resourceType,
                ResourceId = resourceId,
                Metadata = metadata,
                OccurredAt = DateTimeOffset.UtcNow
            });
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex,
                "Failed to publish audit event for action {Action} on {ResourceType}/{ResourceId}",
                action, resourceType, resourceId);
        }
    }
}
