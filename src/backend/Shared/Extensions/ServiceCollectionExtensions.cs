using System.Text;
using CenterPointInbox.Shared.Middleware;
using CenterPointInbox.Shared.Security;
using MassTransit;
using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Diagnostics.HealthChecks;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Diagnostics.HealthChecks;
using Microsoft.IdentityModel.Tokens;

namespace CenterPointInbox.Shared.Extensions;

/// <summary>
/// Extension methods for IServiceCollection and IApplicationBuilder to wire up
/// shared infrastructure across all Center Point Inbox microservices.
/// </summary>
public static class ServiceCollectionExtensions
{
    /// <summary>
    /// Configures JWT Bearer authentication using settings from the "Jwt" configuration section.
    /// Validates issuer, audience, lifetime, and signing key.
    /// </summary>
    public static IServiceCollection AddSharedAuthentication(this IServiceCollection services, IConfiguration configuration)
    {
        var jwtConfig = configuration.GetSection(JwtConfig.SectionName).Get<JwtConfig>()
            ?? throw new InvalidOperationException(
                $"JWT configuration section '{JwtConfig.SectionName}' is missing or empty. " +
                "Ensure appsettings.json contains a valid 'Jwt' section.");

        services.Configure<JwtConfig>(configuration.GetSection(JwtConfig.SectionName));

        var signingKeyBytes = Encoding.UTF8.GetBytes(jwtConfig.SigningKey);

        services.AddAuthentication(options =>
        {
            options.DefaultAuthenticateScheme = JwtBearerDefaults.AuthenticationScheme;
            options.DefaultChallengeScheme = JwtBearerDefaults.AuthenticationScheme;
        })
        .AddJwtBearer(options =>
        {
            options.TokenValidationParameters = new TokenValidationParameters
            {
                ValidateIssuer = true,
                ValidIssuer = jwtConfig.Issuer,

                ValidateAudience = true,
                ValidAudience = jwtConfig.Audience,

                ValidateLifetime = true,
                ClockSkew = TimeSpan.FromSeconds(30),

                ValidateIssuerSigningKey = true,
                IssuerSigningKey = new SymmetricSecurityKey(signingKeyBytes),

                RequireExpirationTime = true,
                RequireSignedTokens = true
            };

            options.Events = new JwtBearerEvents
            {
                OnAuthenticationFailed = context =>
                {
                    if (context.Exception is SecurityTokenExpiredException)
                    {
                        context.Response.Headers["X-Token-Expired"] = "true";
                    }
                    return Task.CompletedTask;
                }
            };
        });

        services.AddAuthorization();

        return services;
    }

    /// <summary>
    /// Configures CORS with allowed origins read from the "Cors:AllowedOrigins" configuration section.
    /// Expects a semicolon-separated list of origins (e.g., "https://app.centerpoint.com;https://localhost:3000").
    /// </summary>
    public static IServiceCollection AddSharedCors(this IServiceCollection services, IConfiguration configuration)
    {
        var allowedOriginsRaw = configuration.GetValue<string>("Cors:AllowedOrigins") ?? "*";

        var allowedOrigins = allowedOriginsRaw
            .Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);

        services.AddCors(options =>
        {
            options.AddDefaultPolicy(policy =>
            {
                if (allowedOrigins.Length == 1 && allowedOrigins[0] == "*")
                {
                    policy.AllowAnyOrigin();
                }
                else
                {
                    policy.WithOrigins(allowedOrigins)
                          .AllowCredentials();
                }

                policy.AllowAnyHeader()
                      .AllowAnyMethod()
                      .WithExposedHeaders(
                          CorrelationIdMiddleware.HeaderName,
                          "X-Token-Expired",
                          "X-Pagination");
            });
        });

        return services;
    }

    /// <summary>
    /// Adds a basic health check endpoint at /health that returns service status.
    /// Microservices can add additional checks (database, Redis, RabbitMQ) on top of this.
    /// </summary>
    public static IServiceCollection AddSharedHealthChecks(this IServiceCollection services)
    {
        services.AddHealthChecks()
            .AddCheck("self", () => HealthCheckResult.Healthy("Service is running."), tags: ["ready", "live"]);

        return services;
    }

    /// <summary>
    /// Maps the shared health check endpoints:
    /// - /health/ready  - readiness probe (all checks tagged "ready")
    /// - /health/live   - liveness probe (all checks tagged "live")
    /// - /health        - all registered checks
    /// </summary>
    public static IApplicationBuilder UseSharedHealthChecks(this IApplicationBuilder app)
    {
        app.UseHealthChecks("/health", new HealthCheckOptions
        {
            Predicate = _ => true,
            ResponseWriter = WriteHealthCheckResponse
        });

        app.UseHealthChecks("/health/ready", new HealthCheckOptions
        {
            Predicate = check => check.Tags.Contains("ready"),
            ResponseWriter = WriteHealthCheckResponse
        });

        app.UseHealthChecks("/health/live", new HealthCheckOptions
        {
            Predicate = check => check.Tags.Contains("live"),
            ResponseWriter = WriteHealthCheckResponse
        });

        return app;
    }

    /// <summary>
    /// Configures MassTransit with RabbitMQ transport using settings from the "RabbitMQ" configuration section.
    /// Expects Host, Username, and Password configuration keys.
    /// An optional configureConsumers action allows each microservice to register its own consumers.
    /// </summary>
    public static IServiceCollection AddSharedMessaging(
        this IServiceCollection services,
        IConfiguration configuration,
        Action<IBusRegistrationConfigurator>? configureConsumers = null)
    {
        services.AddMassTransit(busConfig =>
        {
            busConfig.SetKebabCaseEndpointNameFormatter();

            configureConsumers?.Invoke(busConfig);

            busConfig.UsingRabbitMq((context, rabbitConfig) =>
            {
                var host = configuration.GetValue<string>("RabbitMQ:Host") ?? "localhost";
                var username = configuration.GetValue<string>("RabbitMQ:Username") ?? "guest";
                var password = configuration.GetValue<string>("RabbitMQ:Password") ?? "guest";
                var virtualHost = configuration.GetValue<string>("RabbitMQ:VirtualHost") ?? "/";
                var port = configuration.GetValue<ushort?>("RabbitMQ:Port") ?? 5672;

                rabbitConfig.Host(host, port, virtualHost, h =>
                {
                    h.Username(username);
                    h.Password(password);
                });

                // Configure retry policy for transient failures
                rabbitConfig.UseMessageRetry(retryConfig =>
                {
                    retryConfig.Exponential(
                        retryLimit: 5,
                        minInterval: TimeSpan.FromSeconds(1),
                        maxInterval: TimeSpan.FromSeconds(30),
                        intervalDelta: TimeSpan.FromSeconds(2));
                });

                rabbitConfig.ConfigureEndpoints(context);
            });
        });

        return services;
    }

    /// <summary>
    /// Registers the shared middleware pipeline (correlation ID + exception handling)
    /// in the correct order.
    /// </summary>
    public static IApplicationBuilder UseSharedMiddleware(this IApplicationBuilder app)
    {
        app.UseMiddleware<CorrelationIdMiddleware>();
        app.UseMiddleware<ExceptionHandlingMiddleware>();
        return app;
    }

    private static async Task WriteHealthCheckResponse(HttpContext context, HealthReport report)
    {
        context.Response.ContentType = "application/json";

        var response = new
        {
            status = report.Status.ToString(),
            duration = report.TotalDuration.TotalMilliseconds,
            checks = report.Entries.Select(entry => new
            {
                name = entry.Key,
                status = entry.Value.Status.ToString(),
                description = entry.Value.Description,
                duration = entry.Value.Duration.TotalMilliseconds,
                exception = entry.Value.Exception?.Message
            }),
            timestamp = DateTimeOffset.UtcNow
        };

        await context.Response.WriteAsJsonAsync(response);
    }
}
