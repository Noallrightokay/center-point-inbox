using CenterPointInbox.ApiGateway.Configuration;
using CenterPointInbox.Shared.Models;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Options;

namespace CenterPointInbox.ApiGateway.Controllers;

/// <summary>
/// Aggregated health check controller that pings all downstream microservices
/// and reports their collective health status. Provides a single endpoint for
/// infrastructure monitoring to verify the entire platform is operational.
/// </summary>
[ApiController]
[Route("api/gateway")]
[AllowAnonymous]
public class HealthController : ControllerBase
{
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly GatewaySettings _settings;
    private readonly ILogger<HealthController> _logger;

    public HealthController(
        IHttpClientFactory httpClientFactory,
        IOptions<GatewaySettings> settings,
        ILogger<HealthController> logger)
    {
        _httpClientFactory = httpClientFactory;
        _settings = settings.Value;
        _logger = logger;
    }

    /// <summary>
    /// Returns aggregated health status of all downstream services.
    /// Each service is pinged at its /health/live endpoint.
    /// </summary>
    [HttpGet("health")]
    [ProducesResponseType(typeof(ApiResponse<AggregatedHealthResponse>), StatusCodes.Status200OK)]
    [ProducesResponseType(typeof(ApiResponse<AggregatedHealthResponse>), StatusCodes.Status503ServiceUnavailable)]
    public async Task<IActionResult> GetAggregatedHealth(CancellationToken cancellationToken)
    {
        var services = GetDownstreamServices();
        var healthChecks = new List<ServiceHealthResult>();
        var overallHealthy = true;

        var tasks = services.Select(async service =>
        {
            var result = await CheckServiceHealthAsync(service.Name, service.BaseUrl, cancellationToken);
            return result;
        });

        var results = await Task.WhenAll(tasks);

        foreach (var result in results)
        {
            healthChecks.Add(result);
            if (result.Status != ServiceHealthStatus.Healthy)
            {
                overallHealthy = false;
            }
        }

        var response = new AggregatedHealthResponse
        {
            Status = overallHealthy ? "Healthy" : "Degraded",
            TotalServices = healthChecks.Count,
            HealthyServices = healthChecks.Count(h => h.Status == ServiceHealthStatus.Healthy),
            UnhealthyServices = healthChecks.Count(h => h.Status != ServiceHealthStatus.Healthy),
            Services = healthChecks,
            CheckedAt = DateTimeOffset.UtcNow
        };

        var correlationId = HttpContext.Items.TryGetValue("CorrelationId", out var cid)
            ? cid as string
            : null;

        var apiResponse = ApiResponse<AggregatedHealthResponse>.Success(response, correlationId);

        return overallHealthy
            ? Ok(apiResponse)
            : StatusCode(StatusCodes.Status503ServiceUnavailable, apiResponse);
    }

    /// <summary>
    /// Simple liveness check for the gateway itself.
    /// </summary>
    [HttpGet("ping")]
    [ProducesResponseType(typeof(object), StatusCodes.Status200OK)]
    public IActionResult Ping()
    {
        return Ok(new
        {
            service = "ApiGateway",
            status = "alive",
            timestamp = DateTimeOffset.UtcNow
        });
    }

    /// <summary>
    /// Pings a single downstream service's health endpoint and returns the result.
    /// </summary>
    private async Task<ServiceHealthResult> CheckServiceHealthAsync(
        string serviceName, string baseUrl, CancellationToken cancellationToken)
    {
        var result = new ServiceHealthResult
        {
            Name = serviceName,
            Url = baseUrl
        };

        try
        {
            var client = _httpClientFactory.CreateClient("HealthCheck");
            client.Timeout = TimeSpan.FromSeconds(_settings.HealthCheckTimeoutSeconds);

            var healthUrl = $"{baseUrl.TrimEnd('/')}/health/live";

            var stopwatch = System.Diagnostics.Stopwatch.StartNew();
            var response = await client.GetAsync(healthUrl, cancellationToken);
            stopwatch.Stop();

            result.ResponseTimeMs = stopwatch.Elapsed.TotalMilliseconds;
            result.StatusCode = (int)response.StatusCode;

            if (response.IsSuccessStatusCode)
            {
                result.Status = ServiceHealthStatus.Healthy;
                result.Message = "Service is healthy.";
            }
            else
            {
                result.Status = ServiceHealthStatus.Unhealthy;
                result.Message = $"Service returned HTTP {(int)response.StatusCode}.";
                _logger.LogWarning(
                    "Downstream service {ServiceName} at {Url} returned {StatusCode}",
                    serviceName, healthUrl, (int)response.StatusCode);
            }
        }
        catch (TaskCanceledException)
        {
            result.Status = ServiceHealthStatus.Unhealthy;
            result.Message = $"Health check timed out after {_settings.HealthCheckTimeoutSeconds}s.";
            _logger.LogWarning(
                "Health check timed out for service {ServiceName} at {Url}",
                serviceName, baseUrl);
        }
        catch (HttpRequestException ex)
        {
            result.Status = ServiceHealthStatus.Unhealthy;
            result.Message = $"Connection failed: {ex.Message}";
            _logger.LogWarning(
                ex,
                "Health check failed for service {ServiceName} at {Url}",
                serviceName, baseUrl);
        }
        catch (Exception ex)
        {
            result.Status = ServiceHealthStatus.Unhealthy;
            result.Message = $"Unexpected error: {ex.Message}";
            _logger.LogError(
                ex,
                "Unexpected error during health check for service {ServiceName} at {Url}",
                serviceName, baseUrl);
        }

        return result;
    }

    /// <summary>
    /// Returns the list of downstream services to check, built from configuration.
    /// </summary>
    private List<DownstreamServiceInfo> GetDownstreamServices()
    {
        return
        [
            new() { Name = "AuthService", BaseUrl = _settings.Services.AuthService },
            new() { Name = "GoogleProviderService", BaseUrl = _settings.Services.GoogleProviderService },
            new() { Name = "MicrosoftProviderService", BaseUrl = _settings.Services.MicrosoftProviderService },
            new() { Name = "MergerService", BaseUrl = _settings.Services.MergerService },
            new() { Name = "AiAssistantService", BaseUrl = _settings.Services.AiAssistantService },
            new() { Name = "AuditDlpService", BaseUrl = _settings.Services.AuditDlpService }
        ];
    }
}

#region Response Models

/// <summary>
/// Aggregated health response containing the status of all downstream services.
/// </summary>
public class AggregatedHealthResponse
{
    public string Status { get; set; } = string.Empty;
    public int TotalServices { get; set; }
    public int HealthyServices { get; set; }
    public int UnhealthyServices { get; set; }
    public List<ServiceHealthResult> Services { get; set; } = [];
    public DateTimeOffset CheckedAt { get; set; }
}

/// <summary>
/// Health check result for a single downstream service.
/// </summary>
public class ServiceHealthResult
{
    public string Name { get; set; } = string.Empty;
    public string Url { get; set; } = string.Empty;
    public ServiceHealthStatus Status { get; set; }
    public string? Message { get; set; }
    public int? StatusCode { get; set; }
    public double? ResponseTimeMs { get; set; }
}

public enum ServiceHealthStatus
{
    Healthy,
    Unhealthy
}

/// <summary>
/// Internal model for downstream service discovery.
/// </summary>
internal class DownstreamServiceInfo
{
    public string Name { get; set; } = string.Empty;
    public string BaseUrl { get; set; } = string.Empty;
}

#endregion
