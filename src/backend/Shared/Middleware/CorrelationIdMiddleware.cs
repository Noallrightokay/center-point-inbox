using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging;

namespace CenterPointInbox.Shared.Middleware;

/// <summary>
/// Middleware that extracts or generates a correlation ID for every request.
/// The correlation ID is propagated via the X-Correlation-Id header and added
/// to the logging scope so all log entries for a request are traceable.
/// </summary>
public class CorrelationIdMiddleware
{
    public const string HeaderName = "X-Correlation-Id";
    public const string LogPropertyName = "CorrelationId";

    private readonly RequestDelegate _next;
    private readonly ILogger<CorrelationIdMiddleware> _logger;

    public CorrelationIdMiddleware(RequestDelegate next, ILogger<CorrelationIdMiddleware> logger)
    {
        _next = next;
        _logger = logger;
    }

    public async Task InvokeAsync(HttpContext context)
    {
        var correlationId = GetOrGenerateCorrelationId(context);

        // Store in HttpContext.Items so downstream middleware and controllers can access it
        context.Items[LogPropertyName] = correlationId;

        // Add the correlation ID to the response headers
        context.Response.OnStarting(() =>
        {
            if (!context.Response.Headers.ContainsKey(HeaderName))
            {
                context.Response.Headers[HeaderName] = correlationId;
            }
            return Task.CompletedTask;
        });

        // Add the correlation ID to the logging scope for structured logging
        using (_logger.BeginScope(new Dictionary<string, object>
        {
            [LogPropertyName] = correlationId
        }))
        {
            _logger.LogDebug("Request {Method} {Path} assigned CorrelationId {CorrelationId}",
                context.Request.Method,
                context.Request.Path,
                correlationId);

            await _next(context);
        }
    }

    private static string GetOrGenerateCorrelationId(HttpContext context)
    {
        if (context.Request.Headers.TryGetValue(HeaderName, out var existingId)
            && !string.IsNullOrWhiteSpace(existingId))
        {
            return existingId.ToString();
        }

        return Guid.NewGuid().ToString("D");
    }
}

/// <summary>
/// Extension methods for retrieving the correlation ID from HttpContext.
/// </summary>
public static class CorrelationIdExtensions
{
    /// <summary>
    /// Retrieves the correlation ID from the current HttpContext.
    /// Returns null if no correlation ID has been set.
    /// </summary>
    public static string? GetCorrelationId(this HttpContext context)
    {
        return context.Items.TryGetValue(CorrelationIdMiddleware.LogPropertyName, out var value)
            ? value as string
            : null;
    }
}
