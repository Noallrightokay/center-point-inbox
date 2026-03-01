using System.Text;
using CenterPointInbox.Shared.Contracts;
using CenterPointInbox.TranslationWorker.Configuration;
using CenterPointInbox.TranslationWorker.Models;
using MassTransit;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace CenterPointInbox.TranslationWorker.Services;

/// <summary>
/// MassTransit consumer that processes TranslationRequested messages from RabbitMQ.
/// Orchestrates the full translation pipeline: download source, convert format,
/// optionally redact PHI, upload result, and publish completion event.
/// </summary>
public class TranslationRequestedConsumer : IConsumer<TranslationRequested>
{
    private readonly TranslationDbContext _dbContext;
    private readonly IDocumentConverter _documentConverter;
    private readonly IStorageService _storageService;
    private readonly IPhiRedactor _phiRedactor;
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly TranslationSettings _settings;
    private readonly ILogger<TranslationRequestedConsumer> _logger;

    public TranslationRequestedConsumer(
        TranslationDbContext dbContext,
        IDocumentConverter documentConverter,
        IStorageService storageService,
        IPhiRedactor phiRedactor,
        IHttpClientFactory httpClientFactory,
        IOptions<TranslationSettings> settings,
        ILogger<TranslationRequestedConsumer> logger)
    {
        _dbContext = dbContext;
        _documentConverter = documentConverter;
        _storageService = storageService;
        _phiRedactor = phiRedactor;
        _httpClientFactory = httpClientFactory;
        _settings = settings.Value;
        _logger = logger;
    }

    public async Task Consume(ConsumeContext<TranslationRequested> context)
    {
        var message = context.Message;

        _logger.LogInformation(
            "Received TranslationRequested message. SourceFileId: {SourceFileId}, TenantId: {TenantId}, " +
            "SourceFormat: {SourceFormat}, TargetFormat: {TargetFormat}, RedactPhi: {RedactPhi}",
            message.SourceFileId, message.TenantId, message.SourceFormat,
            message.TargetFormat, message.RedactPhi);

        // Create or find the translation job record
        var job = await GetOrCreateJobAsync(message);

        try
        {
            // Step 1: Update job status to Processing
            await UpdateJobStatusAsync(job, TranslationJobStatus.Processing);

            _logger.LogInformation("Job {JobId}: Status updated to Processing", job.Id);

            // Step 2: Validate the conversion is supported
            if (!_documentConverter.IsConversionSupported(message.SourceFormat, message.TargetFormat))
            {
                throw new NotSupportedException(
                    $"Conversion from '{message.SourceFormat}' to '{message.TargetFormat}' is not supported.");
            }

            // Step 3: Resolve the file metadata to determine the provider and download URL
            var fileMetadata = await _dbContext.FilesMetadata
                .FirstOrDefaultAsync(f => f.Id == message.SourceFileId && f.TenantId == message.TenantId);

            if (fileMetadata is null)
            {
                throw new KeyNotFoundException(
                    $"File metadata not found for SourceFileId '{message.SourceFileId}' in tenant '{message.TenantId}'.");
            }

            // Step 4: Download the source file from the appropriate provider service
            _logger.LogInformation(
                "Job {JobId}: Downloading source file from provider '{Provider}'",
                job.Id, fileMetadata.Provider);

            var sourceBytes = await DownloadSourceFileAsync(fileMetadata);
            job.SourceFileSizeBytes = sourceBytes.Length;

            _logger.LogInformation(
                "Job {JobId}: Downloaded source file, size: {Size} bytes",
                job.Id, sourceBytes.Length);

            // Step 5: Convert the document format
            _logger.LogInformation(
                "Job {JobId}: Converting from {SourceFormat} to {TargetFormat}",
                job.Id, message.SourceFormat, message.TargetFormat);

            var convertedBytes = await _documentConverter.ConvertAsync(
                sourceBytes, message.SourceFormat, message.TargetFormat);

            _logger.LogInformation(
                "Job {JobId}: Conversion completed, output size: {Size} bytes",
                job.Id, convertedBytes.Length);

            // Step 6: Optionally redact PHI from text-based output
            if (message.RedactPhi && IsTextBasedFormat(message.TargetFormat))
            {
                _logger.LogInformation("Job {JobId}: Redacting PHI from output", job.Id);

                var textContent = Encoding.UTF8.GetString(convertedBytes);
                var redactedText = await _phiRedactor.RedactTextAsync(textContent);
                convertedBytes = Encoding.UTF8.GetBytes(redactedText);

                _logger.LogInformation(
                    "Job {JobId}: PHI redaction completed, output size after redaction: {Size} bytes",
                    job.Id, convertedBytes.Length);
            }

            // Step 7: Upload the result to S3
            var outputKey = GenerateOutputKey(job.Id, message.TenantId, message.TargetFormat);

            _logger.LogInformation(
                "Job {JobId}: Uploading result to S3 with key '{Key}'",
                job.Id, outputKey);

            await _storageService.UploadAsync(convertedBytes, outputKey, message.TargetFormat);

            // Step 8: Update job status to Completed
            job.OutputPath = outputKey;
            job.OutputFileSizeBytes = convertedBytes.Length;
            await UpdateJobStatusAsync(job, TranslationJobStatus.Completed);

            _logger.LogInformation(
                "Job {JobId}: Translation completed successfully. OutputPath: {OutputPath}",
                job.Id, outputKey);

            // Step 9: Publish TranslationCompleted event
            await context.Publish(new TranslationCompleted
            {
                JobId = job.Id,
                Status = TranslationStatus.Completed,
                OutputPath = outputKey,
                CompletedAt = DateTimeOffset.UtcNow
            });

            _logger.LogInformation("Job {JobId}: Published TranslationCompleted event", job.Id);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex,
                "Job {JobId}: Translation failed. SourceFileId: {SourceFileId}, Error: {Error}",
                job.Id, message.SourceFileId, ex.Message);

            // Update job status to Failed with error message
            job.ErrorMessage = TruncateErrorMessage(ex.ToString());
            await UpdateJobStatusAsync(job, TranslationJobStatus.Failed);

            // Publish failure event
            await context.Publish(new TranslationCompleted
            {
                JobId = job.Id,
                Status = TranslationStatus.Failed,
                Error = TruncateErrorMessage(ex.Message),
                CompletedAt = DateTimeOffset.UtcNow
            });

            _logger.LogInformation(
                "Job {JobId}: Published TranslationCompleted (Failed) event", job.Id);

            // Do not rethrow - the job has been marked as failed and the failure event published.
            // Rethrowing would cause MassTransit retry, which is undesirable for permanent failures.
            // Transient failures (network, S3) are already handled by MassTransit's retry policy.
        }
    }

    private async Task<TranslationJob> GetOrCreateJobAsync(TranslationRequested message)
    {
        // Check for an existing job with this source file to avoid duplicates
        var existingJob = await _dbContext.TranslationJobs
            .FirstOrDefaultAsync(j =>
                j.SourceFileId == message.SourceFileId &&
                j.TenantId == message.TenantId &&
                j.SourceFormat == message.SourceFormat &&
                j.TargetFormat == message.TargetFormat &&
                j.Status == TranslationJobStatus.Queued);

        if (existingJob is not null)
        {
            _logger.LogInformation(
                "Found existing queued job {JobId} for SourceFileId {SourceFileId}",
                existingJob.Id, message.SourceFileId);
            return existingJob;
        }

        var job = new TranslationJob
        {
            Id = Guid.NewGuid(),
            TenantId = message.TenantId,
            UserId = message.UserId,
            SourceFileId = message.SourceFileId,
            SourceFormat = message.SourceFormat,
            TargetFormat = message.TargetFormat,
            RedactPhi = message.RedactPhi,
            Status = TranslationJobStatus.Queued,
            CreatedAt = DateTimeOffset.UtcNow,
            UpdatedAt = DateTimeOffset.UtcNow
        };

        _dbContext.TranslationJobs.Add(job);
        await _dbContext.SaveChangesAsync();

        _logger.LogInformation("Created new translation job {JobId}", job.Id);
        return job;
    }

    private async Task UpdateJobStatusAsync(TranslationJob job, TranslationJobStatus status)
    {
        job.Status = status;
        job.UpdatedAt = DateTimeOffset.UtcNow;

        if (status == TranslationJobStatus.Processing)
        {
            job.StartedAt = DateTimeOffset.UtcNow;
        }
        else if (status is TranslationJobStatus.Completed or TranslationJobStatus.Failed)
        {
            job.CompletedAt = DateTimeOffset.UtcNow;
        }

        await _dbContext.SaveChangesAsync();
    }

    private async Task<byte[]> DownloadSourceFileAsync(FileMetadata fileMetadata)
    {
        var providerBaseUrl = ResolveProviderBaseUrl(fileMetadata.Provider);

        var downloadUrl = $"{providerBaseUrl.TrimEnd('/')}/api/files/{fileMetadata.Id}/download";

        var httpClient = _httpClientFactory.CreateClient("ProviderService");
        httpClient.Timeout = TimeSpan.FromSeconds(_settings.DownloadTimeoutSeconds);

        var response = await httpClient.GetAsync(downloadUrl);
        response.EnsureSuccessStatusCode();

        var content = await response.Content.ReadAsByteArrayAsync();

        if (content.Length == 0)
        {
            throw new InvalidOperationException(
                $"Downloaded file from provider '{fileMetadata.Provider}' is empty.");
        }

        if (content.Length > _settings.MaxFileSizeBytes)
        {
            throw new InvalidOperationException(
                $"Downloaded file exceeds maximum allowed size of {_settings.MaxFileSizeBytes} bytes. " +
                $"Actual size: {content.Length} bytes.");
        }

        return content;
    }

    private string ResolveProviderBaseUrl(string provider)
    {
        return provider.ToLowerInvariant() switch
        {
            "google" => _settings.Providers.GoogleProviderService,
            "microsoft" => _settings.Providers.MicrosoftProviderService,
            _ => throw new NotSupportedException(
                $"Provider '{provider}' is not supported. Supported providers: google, microsoft.")
        };
    }

    private static string GenerateOutputKey(Guid jobId, Guid tenantId, string targetFormat)
    {
        var extension = GetFileExtension(targetFormat);
        return $"translations/{tenantId:D}/{jobId:D}/output{extension}";
    }

    private static string GetFileExtension(string mimeType)
    {
        return mimeType.ToLowerInvariant() switch
        {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => ".docx",
            "text/html" => ".html",
            "text/csv" => ".csv",
            "application/pdf" => ".pdf",
            _ => ".bin"
        };
    }

    private static bool IsTextBasedFormat(string mimeType)
    {
        return mimeType.ToLowerInvariant() switch
        {
            "text/html" => true,
            "text/csv" => true,
            "text/plain" => true,
            _ => false
        };
    }

    private static string TruncateErrorMessage(string message, int maxLength = 4000)
    {
        if (string.IsNullOrEmpty(message))
            return string.Empty;

        return message.Length <= maxLength
            ? message
            : message[..(maxLength - 3)] + "...";
    }
}
