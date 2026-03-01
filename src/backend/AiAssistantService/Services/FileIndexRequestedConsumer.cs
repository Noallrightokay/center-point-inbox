using System.Text;
using CenterPointInbox.AiAssistantService.Configuration;
using CenterPointInbox.Shared.Contracts;
using MassTransit;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace CenterPointInbox.AiAssistantService.Services;

/// <summary>
/// MassTransit consumer that processes FileIndexRequested messages from RabbitMQ.
/// Downloads file content from the appropriate provider service, extracts text,
/// generates embeddings, and publishes an audit event upon completion.
/// </summary>
public class FileIndexRequestedConsumer : IConsumer<FileIndexRequested>
{
    private readonly IEmbeddingService _embeddingService;
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly AiSettings _settings;
    private readonly ILogger<FileIndexRequestedConsumer> _logger;

    public FileIndexRequestedConsumer(
        IEmbeddingService embeddingService,
        IHttpClientFactory httpClientFactory,
        IOptions<AiSettings> settings,
        ILogger<FileIndexRequestedConsumer> logger)
    {
        _embeddingService = embeddingService;
        _httpClientFactory = httpClientFactory;
        _settings = settings.Value;
        _logger = logger;
    }

    public async Task Consume(ConsumeContext<FileIndexRequested> context)
    {
        var message = context.Message;

        _logger.LogInformation(
            "Received FileIndexRequested message. FileId: {FileId}, TenantId: {TenantId}, " +
            "Provider: {Provider}, UserId: {UserId}",
            message.FileId, message.TenantId, message.Provider, message.UserId);

        try
        {
            // Step 1: Download the file content from the provider service
            var textContent = await DownloadAndExtractTextAsync(message);

            if (string.IsNullOrWhiteSpace(textContent))
            {
                _logger.LogWarning(
                    "FileId {FileId}: No text content could be extracted. Skipping embedding generation.",
                    message.FileId);

                await PublishAuditEventAsync(context, message, "file.index.skipped",
                    new Dictionary<string, string>
                    {
                        ["reason"] = "No extractable text content"
                    });
                return;
            }

            _logger.LogInformation(
                "FileId {FileId}: Extracted {ContentLength} characters of text content",
                message.FileId, textContent.Length);

            // Step 2: Generate embeddings for the extracted text
            await _embeddingService.GenerateEmbeddingsAsync(
                message.TenantId, message.FileId, textContent);

            var chunkCount = await _embeddingService.GetChunkCountAsync(
                message.TenantId, message.FileId);

            _logger.LogInformation(
                "FileId {FileId}: Successfully generated {ChunkCount} embedding chunks",
                message.FileId, chunkCount);

            // Step 3: Publish audit event for successful indexing
            await PublishAuditEventAsync(context, message, "file.indexed",
                new Dictionary<string, string>
                {
                    ["chunkCount"] = chunkCount.ToString(),
                    ["contentLength"] = textContent.Length.ToString()
                });
        }
        catch (Exception ex)
        {
            _logger.LogError(ex,
                "FileId {FileId}: Failed to index file. Error: {Error}",
                message.FileId, ex.Message);

            // Publish audit event for failed indexing
            await PublishAuditEventAsync(context, message, "file.index.failed",
                new Dictionary<string, string>
                {
                    ["error"] = TruncateErrorMessage(ex.Message)
                });

            // Rethrow to let MassTransit handle retries for transient failures
            throw;
        }
    }

    /// <summary>
    /// Downloads file content from the appropriate provider service and extracts text.
    /// Supports basic text extraction from common file formats.
    /// </summary>
    private async Task<string> DownloadAndExtractTextAsync(FileIndexRequested message)
    {
        var providerClientName = ResolveProviderClientName(message.Provider);
        var httpClient = _httpClientFactory.CreateClient(providerClientName);

        var downloadUrl = $"api/files/{message.FileId}/download";

        _logger.LogInformation(
            "FileId {FileId}: Downloading from provider '{Provider}' via {ClientName}",
            message.FileId, message.Provider, providerClientName);

        var response = await httpClient.GetAsync(downloadUrl);
        response.EnsureSuccessStatusCode();

        var contentType = response.Content.Headers.ContentType?.MediaType ?? "application/octet-stream";
        var fileBytes = await response.Content.ReadAsByteArrayAsync();

        if (fileBytes.Length == 0)
        {
            _logger.LogWarning("FileId {FileId}: Downloaded file is empty", message.FileId);
            return string.Empty;
        }

        _logger.LogInformation(
            "FileId {FileId}: Downloaded {Size} bytes, content type: {ContentType}",
            message.FileId, fileBytes.Length, contentType);

        // Extract text based on content type
        return ExtractText(fileBytes, contentType);
    }

    /// <summary>
    /// Extracts text content from file bytes based on the content type.
    /// Handles plain text, HTML (basic tag stripping), and CSV directly.
    /// For binary formats, returns the raw bytes decoded as UTF-8 for best-effort extraction.
    /// </summary>
    internal static string ExtractText(byte[] fileBytes, string contentType)
    {
        return contentType.ToLowerInvariant() switch
        {
            "text/plain" => Encoding.UTF8.GetString(fileBytes),
            "text/csv" => Encoding.UTF8.GetString(fileBytes),
            "text/html" => StripHtmlTags(Encoding.UTF8.GetString(fileBytes)),
            "application/json" => Encoding.UTF8.GetString(fileBytes),
            "text/markdown" => Encoding.UTF8.GetString(fileBytes),
            "application/xml" or "text/xml" => StripXmlTags(Encoding.UTF8.GetString(fileBytes)),
            _ => TryDecodeAsText(fileBytes)
        };
    }

    /// <summary>
    /// Strips HTML tags from content, leaving only the text content.
    /// </summary>
    private static string StripHtmlTags(string html)
    {
        // Remove script and style blocks entirely
        var noScripts = System.Text.RegularExpressions.Regex.Replace(
            html, @"<(script|style)[^>]*>.*?</\1>",
            string.Empty,
            System.Text.RegularExpressions.RegexOptions.Singleline | System.Text.RegularExpressions.RegexOptions.IgnoreCase);

        // Replace block-level tags with newlines for readability
        var withNewlines = System.Text.RegularExpressions.Regex.Replace(
            noScripts, @"</(p|div|br|h[1-6]|li|tr)>",
            "\n",
            System.Text.RegularExpressions.RegexOptions.IgnoreCase);

        // Strip all remaining HTML tags
        var noTags = System.Text.RegularExpressions.Regex.Replace(withNewlines, @"<[^>]+>", string.Empty);

        // Decode HTML entities
        var decoded = System.Net.WebUtility.HtmlDecode(noTags);

        // Collapse multiple whitespace into single spaces per line
        var lines = decoded.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        return string.Join("\n", lines);
    }

    /// <summary>
    /// Strips XML tags from content, leaving only the text content.
    /// </summary>
    private static string StripXmlTags(string xml)
    {
        var noTags = System.Text.RegularExpressions.Regex.Replace(xml, @"<[^>]+>", " ");
        var lines = noTags.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        return string.Join("\n", lines);
    }

    /// <summary>
    /// Attempts to decode binary content as UTF-8 text. If the content appears to be
    /// primarily text (low ratio of replacement characters), returns it; otherwise returns empty.
    /// </summary>
    private static string TryDecodeAsText(byte[] fileBytes)
    {
        try
        {
            var text = Encoding.UTF8.GetString(fileBytes);

            // Check if the content looks like text (less than 5% replacement characters)
            var replacementCount = text.Count(c => c == '\uFFFD');
            var replacementRatio = (double)replacementCount / text.Length;

            if (replacementRatio > 0.05)
            {
                return string.Empty;
            }

            return text;
        }
        catch
        {
            return string.Empty;
        }
    }

    /// <summary>
    /// Resolves the named HttpClient for the given provider.
    /// </summary>
    private static string ResolveProviderClientName(string provider)
    {
        return provider.ToLowerInvariant() switch
        {
            "google" => "GoogleProvider",
            "microsoft" => "MicrosoftProvider",
            _ => throw new NotSupportedException(
                $"Provider '{provider}' is not supported. Supported providers: google, microsoft.")
        };
    }

    /// <summary>
    /// Publishes an AuditEvent message to the message bus.
    /// </summary>
    private static async Task PublishAuditEventAsync(
        ConsumeContext<FileIndexRequested> context,
        FileIndexRequested message,
        string action,
        IDictionary<string, string>? metadata = null)
    {
        await context.Publish(new AuditEvent
        {
            TenantId = message.TenantId,
            UserId = message.UserId,
            Action = action,
            ResourceType = "file",
            ResourceId = message.FileId.ToString(),
            Metadata = metadata,
            OccurredAt = DateTimeOffset.UtcNow
        });
    }

    private static string TruncateErrorMessage(string message, int maxLength = 500)
    {
        if (string.IsNullOrEmpty(message))
            return string.Empty;

        return message.Length <= maxLength
            ? message
            : message[..(maxLength - 3)] + "...";
    }
}
