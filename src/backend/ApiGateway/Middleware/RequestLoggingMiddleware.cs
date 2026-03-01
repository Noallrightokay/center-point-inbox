using System.Diagnostics;
using System.Security.Claims;

namespace CenterPointInbox.ApiGateway.Middleware;

/// <summary>
/// Structured request/response logging middleware.
/// Logs method, path, status code, duration, correlation ID, user ID, and request size.
/// Redacts sensitive header values such as Authorization tokens.
/// </summary>
public class RequestLoggingMiddleware
{
    private readonly RequestDelegate _next;
    private readonly ILogger<RequestLoggingMiddleware> _logger;

    /// <summary>
    /// Headers whose values should be redacted in log output to prevent
    /// accidental credential leakage.
    /// </summary>
    private static readonly HashSet<string> RedactedHeaders = new(StringComparer.OrdinalIgnoreCase)
    {
        "Authorization",
        "Cookie",
        "Set-Cookie",
        "X-Api-Key",
        "X-Auth-Token"
    };

    /// <summary>
    /// Paths that should be excluded from detailed logging to reduce noise
    /// (e.g., health checks and readiness probes).
    /// </summary>
    private static readonly string[] ExcludedPaths =
    [
        "/health",
        "/health/ready",
        "/health/live",
        "/favicon.ico"
    ];

    public RequestLoggingMiddleware(RequestDelegate next, ILogger<RequestLoggingMiddleware> logger)
    {
        _next = next;
        _logger = logger;
    }

    public async Task InvokeAsync(HttpContext context)
    {
        if (ShouldSkipLogging(context.Request.Path))
        {
            await _next(context);
            return;
        }

        var stopwatch = Stopwatch.StartNew();
        var requestSize = context.Request.ContentLength ?? 0;
        var correlationId = ExtractCorrelationId(context);

        _logger.LogInformation(
            "HTTP {Method} {Path} started. CorrelationId: {CorrelationId}, RequestSize: {RequestSize} bytes, Headers: {Headers}",
            context.Request.Method,
            GetRedactedPath(context.Request),
            correlationId,
            requestSize,
            GetRedactedHeaders(context.Request));

        try
        {
            await _next(context);
        }
        finally
        {
            stopwatch.Stop();
            var userId = ExtractUserId(context);
            var durationMs = stopwatch.Elapsed.TotalMilliseconds;
            var statusCode = context.Response.StatusCode;

            var logLevel = statusCode switch
            {
                >= 500 => LogLevel.Error,
                >= 400 => LogLevel.Warning,
                _ => LogLevel.Information
            };

            _logger.Log(
                logLevel,
                "HTTP {Method} {Path} completed {StatusCode} in {DurationMs:F1}ms. " +
                "CorrelationId: {CorrelationId}, UserId: {UserId}, RequestSize: {RequestSize} bytes",
                context.Request.Method,
                GetRedactedPath(context.Request),
                statusCode,
                durationMs,
                correlationId,
                userId ?? "anonymous",
                requestSize);
        }
    }

    /// <summary>
    /// Determines whether the request path is in the exclusion list.
    /// </summary>
    private static bool ShouldSkipLogging(PathString path)
    {
        var pathValue = path.Value ?? string.Empty;
        return Array.Exists(ExcludedPaths, excluded =>
            pathValue.Equals(excluded, StringComparison.OrdinalIgnoreCase));
    }

    /// <summary>
    /// Extracts the correlation ID from HttpContext.Items where the
    /// CorrelationIdMiddleware stores it.
    /// </summary>
    private static string ExtractCorrelationId(HttpContext context)
    {
        return context.Items.TryGetValue("CorrelationId", out var value)
            ? value as string ?? "unknown"
            : "unknown";
    }

    /// <summary>
    /// Extracts the authenticated user ID from JWT claims.
    /// Returns null if the user is not authenticated.
    /// </summary>
    private static string? ExtractUserId(HttpContext context)
    {
        return context.User.FindFirstValue(ClaimTypes.NameIdentifier)
               ?? context.User.FindFirstValue("sub");
    }

    /// <summary>
    /// Returns the request path with query string, suitable for logging.
    /// Does not redact query parameters as they may be needed for debugging,
    /// but sensitive values should not be passed via query strings by convention.
    /// </summary>
    private static string GetRedactedPath(HttpRequest request)
    {
        return request.QueryString.HasValue
            ? $"{request.Path}{request.QueryString}"
            : request.Path.ToString();
    }

    /// <summary>
    /// Produces a dictionary of request headers with sensitive values replaced
    /// by "[REDACTED]" to prevent credential leakage in log output.
    /// </summary>
    private static Dictionary<string, string> GetRedactedHeaders(HttpRequest request)
    {
        var headers = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);

        foreach (var header in request.Headers)
        {
            if (RedactedHeaders.Contains(header.Key))
            {
                headers[header.Key] = "[REDACTED]";
            }
            else
            {
                headers[header.Key] = header.Value.ToString();
            }
        }

        return headers;
    }
}
