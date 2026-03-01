using CenterPointInbox.MergerService.Models;
using CenterPointInbox.MergerService.Services;
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
        .Enrich.WithProperty("ServiceName", "MergerService")
        .WriteTo.Console(
            outputTemplate: "[{Timestamp:HH:mm:ss} {Level:u3}] [{CorrelationId}] {Message:lj}{NewLine}{Exception}");
});

// ---------------------------------------------------------------------------
// Database context (PostgreSQL via Npgsql) - files_metadata and translation_jobs
// ---------------------------------------------------------------------------
builder.Services.AddDbContext<MergerDbContext>(options =>
{
    options.UseNpgsql(
        builder.Configuration.GetConnectionString("MergerDb"),
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
// Named HTTP clients for provider service communication
// ---------------------------------------------------------------------------
builder.Services.AddHttpClient("GoogleProvider", client =>
{
    var baseUrl = builder.Configuration.GetValue<string>("ProviderServices:GoogleBaseUrl")
        ?? "http://localhost:5010";
    client.BaseAddress = new Uri(baseUrl);
    client.Timeout = TimeSpan.FromSeconds(60);
    client.DefaultRequestHeaders.Accept.Add(
        new System.Net.Http.Headers.MediaTypeWithQualityHeaderValue("application/json"));
});

builder.Services.AddHttpClient("MicrosoftProvider", client =>
{
    var baseUrl = builder.Configuration.GetValue<string>("ProviderServices:MicrosoftBaseUrl")
        ?? "http://localhost:5020";
    client.BaseAddress = new Uri(baseUrl);
    client.Timeout = TimeSpan.FromSeconds(60);
    client.DefaultRequestHeaders.Accept.Add(
        new System.Net.Http.Headers.MediaTypeWithQualityHeaderValue("application/json"));
});

// ---------------------------------------------------------------------------
// Application service registrations
// ---------------------------------------------------------------------------
builder.Services.AddScoped<IMergerService, MergerServiceImpl>();
builder.Services.AddScoped<ITranslationService, TranslationService>();

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
Log.Information("MergerService starting on {Environment}", app.Environment.EnvironmentName);

try
{
    app.Run();
}
catch (Exception ex)
{
    Log.Fatal(ex, "MergerService terminated unexpectedly");
}
finally
{
    Log.CloseAndFlush();
}
