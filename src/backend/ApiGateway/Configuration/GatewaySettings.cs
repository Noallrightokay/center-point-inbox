namespace CenterPointInbox.ApiGateway.Configuration;

/// <summary>
/// POCO for API Gateway settings including rate limiting configuration,
/// downstream service URLs, and Redis connection details.
/// Bound from the "Gateway" section of appsettings.json.
/// </summary>
public class GatewaySettings
{
    public const string SectionName = "Gateway";

    /// <summary>
    /// Redis connection string used for distributed rate limiting counters.
    /// </summary>
    public string RedisConnectionString { get; set; } = "localhost:6379";

    /// <summary>
    /// Global rate limit settings applied to all endpoints unless overridden.
    /// </summary>
    public RateLimitSettings GlobalRateLimit { get; set; } = new();

    /// <summary>
    /// Per-endpoint rate limit overrides keyed by endpoint pattern (e.g., "/api/auth/**").
    /// </summary>
    public Dictionary<string, RateLimitSettings> EndpointRateLimits { get; set; } = new();

    /// <summary>
    /// Downstream service base URLs for health check aggregation.
    /// </summary>
    public DownstreamServiceUrls Services { get; set; } = new();

    /// <summary>
    /// Timeout in seconds for downstream health check pings.
    /// </summary>
    public int HealthCheckTimeoutSeconds { get; set; } = 5;
}

/// <summary>
/// Configuration for a sliding window rate limit.
/// </summary>
public class RateLimitSettings
{
    /// <summary>
    /// Maximum number of requests allowed within the window.
    /// </summary>
    public int MaxRequests { get; set; } = 100;

    /// <summary>
    /// Sliding window duration in seconds.
    /// </summary>
    public int WindowSeconds { get; set; } = 60;

    /// <summary>
    /// Whether to include the client IP in the rate limit key
    /// (in addition to the authenticated user ID).
    /// </summary>
    public bool IncludeIpInKey { get; set; } = true;
}

/// <summary>
/// Base URLs for all downstream microservices used for health check aggregation
/// and any direct service-to-service communication.
/// </summary>
public class DownstreamServiceUrls
{
    public string AuthService { get; set; } = "http://auth-service:5001";
    public string GoogleProviderService { get; set; } = "http://google-provider:5002";
    public string MicrosoftProviderService { get; set; } = "http://microsoft-provider:5003";
    public string MergerService { get; set; } = "http://merger-service:5004";
    public string AiAssistantService { get; set; } = "http://ai-assistant:5006";
    public string AuditDlpService { get; set; } = "http://audit-dlp:5007";
}
