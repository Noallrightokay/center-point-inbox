using System.Security.Claims;
using System.Text.Json;
using CenterPointInbox.ApiGateway.Configuration;
using CenterPointInbox.Shared.Models;
using Microsoft.Extensions.Options;
using StackExchange.Redis;

namespace CenterPointInbox.ApiGateway.Middleware;

/// <summary>
/// Per-client rate limiting middleware using a Redis-backed sliding window counter.
/// Extracts client identity from JWT claims or falls back to the remote IP address.
/// Supports per-endpoint pattern overrides configured in GatewaySettings.
/// </summary>
public class RateLimitingMiddleware
{
    private readonly RequestDelegate _next;
    private readonly ILogger<RateLimitingMiddleware> _logger;
    private readonly GatewaySettings _settings;
    private readonly IConnectionMultiplexer _redis;

    private const string RateLimitKeyPrefix = "rl:";

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull
    };

    public RateLimitingMiddleware(
        RequestDelegate next,
        ILogger<RateLimitingMiddleware> logger,
        IOptions<GatewaySettings> settings,
        IConnectionMultiplexer redis)
    {
        _next = next;
        _logger = logger;
        _settings = settings.Value;
        _redis = redis;
    }

    public async Task InvokeAsync(HttpContext context)
    {
        var clientId = ExtractClientId(context);
        var rateLimitSettings = ResolveRateLimitSettings(context.Request.Path);
        var key = BuildRateLimitKey(clientId, context.Request.Path);

        var (isAllowed, currentCount, retryAfterSeconds) = await CheckRateLimitAsync(
            key,
            rateLimitSettings.MaxRequests,
            rateLimitSettings.WindowSeconds);

        // Always set rate limit headers for client visibility
        context.Response.Headers["X-RateLimit-Limit"] = rateLimitSettings.MaxRequests.ToString();
        context.Response.Headers["X-RateLimit-Remaining"] = Math.Max(0, rateLimitSettings.MaxRequests - currentCount).ToString();
        context.Response.Headers["X-RateLimit-Reset"] = retryAfterSeconds.ToString();

        if (!isAllowed)
        {
            _logger.LogWarning(
                "Rate limit exceeded for client {ClientId} on path {Path}. Count: {Count}, Limit: {Limit}, Window: {WindowSeconds}s",
                clientId, context.Request.Path, currentCount, rateLimitSettings.MaxRequests, rateLimitSettings.WindowSeconds);

            context.Response.StatusCode = StatusCodes.Status429TooManyRequests;
            context.Response.Headers["Retry-After"] = retryAfterSeconds.ToString();
            context.Response.ContentType = "application/json";

            var correlationId = context.Items.TryGetValue("CorrelationId", out var cid)
                ? cid as string
                : null;

            var response = ApiResponse<object>.Error(
                "Rate limit exceeded. Please retry after the specified duration.",
                "RATE_LIMIT_EXCEEDED",
                429,
                correlationId);

            var json = JsonSerializer.Serialize(response, JsonOptions);
            await context.Response.WriteAsync(json);
            return;
        }

        await _next(context);
    }

    /// <summary>
    /// Extracts a unique client identifier from the JWT token (sub claim) or falls
    /// back to the remote IP address for unauthenticated requests.
    /// </summary>
    private static string ExtractClientId(HttpContext context)
    {
        var userId = context.User.FindFirstValue(ClaimTypes.NameIdentifier)
                     ?? context.User.FindFirstValue("sub");

        if (!string.IsNullOrEmpty(userId))
        {
            return $"user:{userId}";
        }

        var ipAddress = context.Connection.RemoteIpAddress?.ToString() ?? "unknown";

        // Check for forwarded headers (behind load balancer)
        if (context.Request.Headers.TryGetValue("X-Forwarded-For", out var forwardedFor))
        {
            var firstIp = forwardedFor.ToString().Split(',', StringSplitOptions.TrimEntries).FirstOrDefault();
            if (!string.IsNullOrEmpty(firstIp))
            {
                ipAddress = firstIp;
            }
        }

        return $"ip:{ipAddress}";
    }

    /// <summary>
    /// Resolves the rate limit settings for the given request path by checking
    /// endpoint-specific overrides first, then falling back to the global rate limit.
    /// </summary>
    private RateLimitSettings ResolveRateLimitSettings(PathString requestPath)
    {
        var path = requestPath.Value ?? "/";

        foreach (var (pattern, settings) in _settings.EndpointRateLimits)
        {
            if (MatchesPattern(path, pattern))
            {
                return settings;
            }
        }

        return _settings.GlobalRateLimit;
    }

    /// <summary>
    /// Matches a request path against a pattern supporting ** wildcard suffix.
    /// Example: "/api/auth/**" matches "/api/auth/login", "/api/auth/register", etc.
    /// </summary>
    private static bool MatchesPattern(string path, string pattern)
    {
        if (pattern.EndsWith("/**"))
        {
            var prefix = pattern[..^3];
            return path.StartsWith(prefix, StringComparison.OrdinalIgnoreCase);
        }

        return string.Equals(path, pattern, StringComparison.OrdinalIgnoreCase);
    }

    /// <summary>
    /// Builds the Redis key for the rate limit counter. Uses the client ID and a
    /// normalized path prefix to group related endpoints together.
    /// </summary>
    private static string BuildRateLimitKey(string clientId, PathString requestPath)
    {
        var path = requestPath.Value ?? "/";

        // Normalize path to the first two segments for grouping
        // e.g., /api/auth/login -> /api/auth
        var segments = path.Split('/', StringSplitOptions.RemoveEmptyEntries);
        var normalizedPath = segments.Length >= 2
            ? $"/{segments[0]}/{segments[1]}"
            : path;

        return $"{RateLimitKeyPrefix}{clientId}:{normalizedPath}";
    }

    /// <summary>
    /// Checks the sliding window rate limit using Redis sorted sets.
    /// Each request is scored by its timestamp. Expired entries outside the
    /// window are removed, and the remaining count is checked against the limit.
    /// </summary>
    /// <returns>
    /// A tuple of (isAllowed, currentCount, retryAfterSeconds).
    /// </returns>
    private async Task<(bool IsAllowed, long CurrentCount, int RetryAfterSeconds)> CheckRateLimitAsync(
        string key, int maxRequests, int windowSeconds)
    {
        var db = _redis.GetDatabase();
        var now = DateTimeOffset.UtcNow;
        var windowStart = now.AddSeconds(-windowSeconds);
        var nowTimestamp = now.ToUnixTimeMilliseconds();
        var windowStartTimestamp = windowStart.ToUnixTimeMilliseconds();

        // Use a Lua script for atomicity: remove expired entries, add current request,
        // count entries in window, and set TTL.
        const string luaScript = """
            redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
            local count = redis.call('ZCARD', KEYS[1])
            if count < tonumber(ARGV[3]) then
                redis.call('ZADD', KEYS[1], ARGV[2], ARGV[2] .. ':' .. math.random(1000000))
                redis.call('EXPIRE', KEYS[1], tonumber(ARGV[4]))
                return {1, count + 1}
            else
                local oldest = redis.call('ZRANGE', KEYS[1], 0, 0, 'WITHSCORES')
                local oldestScore = 0
                if #oldest >= 2 then
                    oldestScore = tonumber(oldest[2])
                end
                return {0, count, oldestScore}
            end
            """;

        try
        {
            var result = (RedisResult[]?)await db.ScriptEvaluateAsync(
                luaScript,
                new RedisKey[] { key },
                new RedisValue[]
                {
                    windowStartTimestamp,
                    nowTimestamp,
                    maxRequests,
                    windowSeconds + 1 // TTL slightly longer than window to avoid edge cases
                });

            if (result == null || result.Length < 2)
            {
                // If Redis fails, allow the request (fail-open)
                _logger.LogWarning("Unexpected Redis rate limit result for key {Key}. Allowing request.", key);
                return (true, 0, 0);
            }

            var allowed = (int)result[0] == 1;
            var count = (long)result[1];

            if (!allowed && result.Length >= 3)
            {
                var oldestTimestamp = (long)result[2];
                var oldestTime = DateTimeOffset.FromUnixTimeMilliseconds(oldestTimestamp);
                var retryAfter = (int)Math.Ceiling((oldestTime.AddSeconds(windowSeconds) - now).TotalSeconds);
                return (false, count, Math.Max(1, retryAfter));
            }

            return (allowed, count, allowed ? 0 : windowSeconds);
        }
        catch (RedisException ex)
        {
            // Fail-open: if Redis is unavailable, allow the request and log a warning
            _logger.LogWarning(ex, "Redis rate limiting unavailable for key {Key}. Allowing request (fail-open).", key);
            return (true, 0, 0);
        }
    }
}
