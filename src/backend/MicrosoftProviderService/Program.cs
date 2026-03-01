using CenterPointInbox.MicrosoftProviderService.Configuration;
using CenterPointInbox.MicrosoftProviderService.Models;
using CenterPointInbox.MicrosoftProviderService.Services;
using CenterPointInbox.Shared.Extensions;
using CenterPointInbox.Shared.Security;
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
        .Enrich.WithProperty("ServiceName", "MicrosoftProviderService")
        .WriteTo.Console(
            outputTemplate: "[{Timestamp:HH:mm:ss} {Level:u3}] [{CorrelationId}] {Message:lj}{NewLine}{Exception}");
});

// ---------------------------------------------------------------------------
// Configuration bindings
// ---------------------------------------------------------------------------
builder.Services.Configure<MicrosoftSettings>(
    builder.Configuration.GetSection(MicrosoftSettings.SectionName));

builder.Services.Configure<JwtConfig>(
    builder.Configuration.GetSection(JwtConfig.SectionName));

// ---------------------------------------------------------------------------
// Database context (PostgreSQL via Npgsql)
// ---------------------------------------------------------------------------
builder.Services.AddDbContext<MicrosoftDbContext>(options =>
{
    options.UseNpgsql(
        builder.Configuration.GetConnectionString("MicrosoftProviderDb"),
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
// HTTP client for Microsoft OAuth token refresh
// ---------------------------------------------------------------------------
builder.Services.AddHttpClient("MicrosoftOAuth", client =>
{
    client.Timeout = TimeSpan.FromSeconds(30);
    client.DefaultRequestHeaders.Accept.Add(
        new System.Net.Http.Headers.MediaTypeWithQualityHeaderValue("application/json"));
});

// ---------------------------------------------------------------------------
// Application service registrations
// ---------------------------------------------------------------------------
builder.Services.AddScoped<IMicrosoftGraphService, MicrosoftGraphService>();

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
Log.Information("MicrosoftProviderService starting on {Environment}", app.Environment.EnvironmentName);

try
{
    app.Run();
}
catch (Exception ex)
{
    Log.Fatal(ex, "MicrosoftProviderService terminated unexpectedly");
}
finally
{
    Log.CloseAndFlush();
}
