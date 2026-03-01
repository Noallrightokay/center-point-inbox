using CenterPointInbox.ApiGateway.Configuration;
using CenterPointInbox.ApiGateway.Middleware;
using CenterPointInbox.Shared.Extensions;
using Serilog;
using StackExchange.Redis;

namespace CenterPointInbox.ApiGateway;

public class Program
{
    public static void Main(string[] args)
    {
        // Configure Serilog early for startup logging
        Log.Logger = new LoggerConfiguration()
            .WriteTo.Console()
            .CreateBootstrapLogger();

        try
        {
            Log.Information("Starting Center Point Inbox API Gateway");

            var builder = WebApplication.CreateBuilder(args);

            // ──────────────────────────────────────────────
            // Configuration sources
            // ──────────────────────────────────────────────
            builder.Configuration.AddJsonFile(
                "Configuration/yarp.json",
                optional: false,
                reloadOnChange: true);

            // ──────────────────────────────────────────────
            // Serilog
            // ──────────────────────────────────────────────
            builder.Host.UseSerilog((context, services, loggerConfig) =>
            {
                loggerConfig
                    .ReadFrom.Configuration(context.Configuration)
                    .ReadFrom.Services(services)
                    .Enrich.FromLogContext()
                    .Enrich.WithProperty("Service", "ApiGateway");
            });

            // ──────────────────────────────────────────────
            // Gateway settings
            // ──────────────────────────────────────────────
            builder.Services.Configure<GatewaySettings>(
                builder.Configuration.GetSection(GatewaySettings.SectionName));

            var gatewaySettings = builder.Configuration
                .GetSection(GatewaySettings.SectionName)
                .Get<GatewaySettings>() ?? new GatewaySettings();

            // ──────────────────────────────────────────────
            // Redis (for rate limiting)
            // ──────────────────────────────────────────────
            builder.Services.AddSingleton<IConnectionMultiplexer>(sp =>
            {
                var config = ConfigurationOptions.Parse(gatewaySettings.RedisConnectionString);
                config.AbortOnConnectFail = false;
                config.ConnectRetry = 3;
                config.ConnectTimeout = 5000;
                return ConnectionMultiplexer.Connect(config);
            });

            // ──────────────────────────────────────────────
            // Shared infrastructure (auth, CORS, health checks)
            // ──────────────────────────────────────────────
            builder.Services.AddSharedAuthentication(builder.Configuration);
            builder.Services.AddSharedCors(builder.Configuration);
            builder.Services.AddSharedHealthChecks();

            // ──────────────────────────────────────────────
            // YARP Reverse Proxy
            // ──────────────────────────────────────────────
            builder.Services
                .AddReverseProxy()
                .LoadFromConfig(builder.Configuration.GetSection("ReverseProxy"));

            // ──────────────────────────────────────────────
            // Controllers & Swagger
            // ──────────────────────────────────────────────
            builder.Services.AddControllers();
            builder.Services.AddEndpointsApiExplorer();
            builder.Services.AddSwaggerGen(options =>
            {
                options.SwaggerDoc("v1", new Microsoft.OpenApi.Models.OpenApiInfo
                {
                    Title = "Center Point Inbox - API Gateway",
                    Version = "v1",
                    Description = "API Gateway for the Center Point Inbox platform. " +
                                  "Routes requests to downstream microservices and provides " +
                                  "rate limiting, authentication, and health monitoring."
                });

                // Add JWT bearer auth to Swagger UI
                options.AddSecurityDefinition("Bearer", new Microsoft.OpenApi.Models.OpenApiSecurityScheme
                {
                    Name = "Authorization",
                    Type = Microsoft.OpenApi.Models.SecuritySchemeType.Http,
                    Scheme = "bearer",
                    BearerFormat = "JWT",
                    In = Microsoft.OpenApi.Models.ParameterLocation.Header,
                    Description = "Enter your JWT token"
                });

                options.AddSecurityRequirement(new Microsoft.OpenApi.Models.OpenApiSecurityRequirement
                {
                    {
                        new Microsoft.OpenApi.Models.OpenApiSecurityScheme
                        {
                            Reference = new Microsoft.OpenApi.Models.OpenApiReference
                            {
                                Type = Microsoft.OpenApi.Models.ReferenceType.SecurityScheme,
                                Id = "Bearer"
                            }
                        },
                        Array.Empty<string>()
                    }
                });
            });

            // ──────────────────────────────────────────────
            // HttpClient for downstream health checks
            // ──────────────────────────────────────────────
            builder.Services.AddHttpClient("HealthCheck", client =>
            {
                client.Timeout = TimeSpan.FromSeconds(gatewaySettings.HealthCheckTimeoutSeconds);
                client.DefaultRequestHeaders.Add("User-Agent", "CenterPointInbox-Gateway-HealthCheck");
            });

            // ──────────────────────────────────────────────
            // Build the app
            // ──────────────────────────────────────────────
            var app = builder.Build();

            // ──────────────────────────────────────────────
            // Middleware pipeline (order matters)
            // ──────────────────────────────────────────────

            // 1. Serilog request logging (outermost)
            app.UseSerilogRequestLogging(options =>
            {
                options.EnrichDiagnosticContext = (diagnosticContext, httpContext) =>
                {
                    diagnosticContext.Set("RequestHost", httpContext.Request.Host.Value);
                    diagnosticContext.Set("UserAgent", httpContext.Request.Headers.UserAgent.ToString());

                    if (httpContext.Items.TryGetValue("CorrelationId", out var correlationId))
                    {
                        diagnosticContext.Set("CorrelationId", correlationId);
                    }
                };
            });

            // 2. Swagger (before auth so docs are accessible)
            if (app.Environment.IsDevelopment())
            {
                app.UseSwagger();
                app.UseSwaggerUI(options =>
                {
                    options.SwaggerEndpoint("/swagger/v1/swagger.json", "Center Point Inbox Gateway v1");
                    options.RoutePrefix = "swagger";
                });
            }

            // 3. Shared middleware: correlation ID + exception handling
            app.UseSharedMiddleware();

            // 4. Custom request logging
            app.UseMiddleware<RequestLoggingMiddleware>();

            // 5. Shared health checks
            app.UseSharedHealthChecks();

            // 6. CORS
            app.UseCors();

            // 7. Authentication & Authorization
            app.UseAuthentication();
            app.UseAuthorization();

            // 8. Rate limiting (after auth so we can extract user ID from JWT)
            app.UseMiddleware<RateLimitingMiddleware>();

            // 9. Map controllers (HealthController etc.)
            app.MapControllers();

            // 10. Map YARP reverse proxy routes
            app.MapReverseProxy();

            // ──────────────────────────────────────────────
            // Run
            // ──────────────────────────────────────────────
            app.Run();
        }
        catch (Exception ex)
        {
            Log.Fatal(ex, "API Gateway terminated unexpectedly");
        }
        finally
        {
            Log.CloseAndFlush();
        }
    }
}
