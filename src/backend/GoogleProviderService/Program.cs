using CenterPointInbox.GoogleProviderService.Configuration;
using CenterPointInbox.GoogleProviderService.Models;
using CenterPointInbox.GoogleProviderService.Services;
using CenterPointInbox.Shared.Extensions;
using Microsoft.EntityFrameworkCore;
using Serilog;

var builder = WebApplication.CreateBuilder(args);

// ---------------------------------------------------------------------------
// Serilog configuration
// ---------------------------------------------------------------------------
builder.Host.UseSerilog((context, loggerConfig) =>
{
    loggerConfig
        .ReadFrom.Configuration(context.Configuration)
        .Enrich.FromLogContext()
        .Enrich.WithProperty("ServiceName", "GoogleProviderService")
        .WriteTo.Console(
            outputTemplate: "[{Timestamp:HH:mm:ss} {Level:u3}] [{CorrelationId}] {Message:lj}{NewLine}{Exception}");
});

// ---------------------------------------------------------------------------
// Configuration bindings
// ---------------------------------------------------------------------------
builder.Services.Configure<GoogleSettings>(
    builder.Configuration.GetSection(GoogleSettings.SectionName));

// ---------------------------------------------------------------------------
// Database context (PostgreSQL via Npgsql) - read-only for OAuth tokens and file metadata
// ---------------------------------------------------------------------------
builder.Services.AddDbContext<GoogleDbContext>(options =>
{
    options.UseNpgsql(
        builder.Configuration.GetConnectionString("GoogleProviderDb"),
        npgsqlOptions =>
        {
            npgsqlOptions.EnableRetryOnFailure(
                maxRetryCount: 3,
                maxRetryDelay: TimeSpan.FromSeconds(10),
                errorCodesToAdd: null);
            npgsqlOptions.CommandTimeout(30);
        });
});

// ---------------------------------------------------------------------------
// Shared infrastructure (authentication, CORS, health checks, messaging)
// ---------------------------------------------------------------------------
builder.Services.AddSharedAuthentication(builder.Configuration);
builder.Services.AddSharedCors(builder.Configuration);
builder.Services.AddSharedHealthChecks();
builder.Services.AddSharedMessaging(builder.Configuration);

// ---------------------------------------------------------------------------
// HTTP client for Google OAuth token refresh
// ---------------------------------------------------------------------------
builder.Services.AddHttpClient("GoogleAuth", client =>
{
    client.Timeout = TimeSpan.FromSeconds(30);
    client.DefaultRequestHeaders.Accept.Add(
        new System.Net.Http.Headers.MediaTypeWithQualityHeaderValue("application/json"));
});

// ---------------------------------------------------------------------------
// Application service registrations
// ---------------------------------------------------------------------------
builder.Services.AddScoped<IGoogleDriveService, GoogleDriveService>();

// ---------------------------------------------------------------------------
// Controllers
// ---------------------------------------------------------------------------
builder.Services.AddControllers();

// ---------------------------------------------------------------------------
// Build and configure the middleware pipeline
// ---------------------------------------------------------------------------
var app = builder.Build();

// Shared middleware: correlation ID + global exception handling
app.UseSharedMiddleware();

// Health check endpoints
app.UseSharedHealthChecks();

app.UseHttpsRedirection();

app.UseCors();

app.UseAuthentication();
app.UseAuthorization();

app.MapControllers();

// ---------------------------------------------------------------------------
// Start the application
// ---------------------------------------------------------------------------
Log.Information("GoogleProviderService starting on {Environment}", app.Environment.EnvironmentName);

try
{
    app.Run();
}
catch (Exception ex)
{
    Log.Fatal(ex, "GoogleProviderService terminated unexpectedly");
}
finally
{
    Log.CloseAndFlush();
}
