using CenterPointInbox.AiAssistantService.Configuration;
using CenterPointInbox.AiAssistantService.Models;
using CenterPointInbox.AiAssistantService.Services;
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
        .Enrich.WithProperty("ServiceName", "AiAssistantService")
        .WriteTo.Console(
            outputTemplate: "[{Timestamp:HH:mm:ss} {Level:u3}] [{CorrelationId}] {Message:lj}{NewLine}{Exception}");
});

// ---------------------------------------------------------------------------
// Configuration bindings
// ---------------------------------------------------------------------------
builder.Services.Configure<AiSettings>(
    builder.Configuration.GetSection(AiSettings.SectionName));

var aiSettings = builder.Configuration
    .GetSection(AiSettings.SectionName)
    .Get<AiSettings>() ?? new AiSettings();

// ---------------------------------------------------------------------------
// Database context (PostgreSQL via Npgsql with pgvector support)
// ---------------------------------------------------------------------------
builder.Services.AddDbContext<AiDbContext>(options =>
{
    options.UseNpgsql(
        builder.Configuration.GetConnectionString("AiDb"),
        npgsqlOptions =>
        {
            npgsqlOptions.UseVector();
            npgsqlOptions.EnableRetryOnFailure(
                maxRetryCount: 3,
                maxRetryDelay: TimeSpan.FromSeconds(10),
                errorCodesToAdd: null);
            npgsqlOptions.CommandTimeout(30);
        });
});

// ---------------------------------------------------------------------------
// Shared infrastructure (authentication, CORS, health checks)
// ---------------------------------------------------------------------------
builder.Services.AddSharedAuthentication(builder.Configuration);
builder.Services.AddSharedCors(builder.Configuration);
builder.Services.AddSharedHealthChecks();

// ---------------------------------------------------------------------------
// MassTransit messaging with FileIndexRequestedConsumer
// ---------------------------------------------------------------------------
builder.Services.AddSharedMessaging(builder.Configuration, busConfig =>
{
    busConfig.AddConsumer<FileIndexRequestedConsumer>();
});

// ---------------------------------------------------------------------------
// Named HTTP client for OpenAI API
// ---------------------------------------------------------------------------
builder.Services.AddHttpClient("OpenAI", client =>
{
    client.BaseAddress = new Uri("https://api.openai.com/");
    client.Timeout = TimeSpan.FromSeconds(120);
    client.DefaultRequestHeaders.Add("Authorization", $"Bearer {aiSettings.OpenAiApiKey}");
    client.DefaultRequestHeaders.Accept.Add(
        new System.Net.Http.Headers.MediaTypeWithQualityHeaderValue("application/json"));
});

// ---------------------------------------------------------------------------
// Named HTTP clients for provider service communication (file downloads)
// ---------------------------------------------------------------------------
builder.Services.AddHttpClient("GoogleProvider", client =>
{
    var baseUrl = builder.Configuration.GetValue<string>("ProviderServices:GoogleBaseUrl")
        ?? "http://localhost:5010";
    client.BaseAddress = new Uri(baseUrl);
    client.Timeout = TimeSpan.FromSeconds(60);
    client.DefaultRequestHeaders.Accept.Add(
        new System.Net.Http.Headers.MediaTypeWithQualityHeaderValue("application/octet-stream"));
});

builder.Services.AddHttpClient("MicrosoftProvider", client =>
{
    var baseUrl = builder.Configuration.GetValue<string>("ProviderServices:MicrosoftBaseUrl")
        ?? "http://localhost:5020";
    client.BaseAddress = new Uri(baseUrl);
    client.Timeout = TimeSpan.FromSeconds(60);
    client.DefaultRequestHeaders.Accept.Add(
        new System.Net.Http.Headers.MediaTypeWithQualityHeaderValue("application/octet-stream"));
});

// ---------------------------------------------------------------------------
// Application service registrations
// ---------------------------------------------------------------------------
builder.Services.AddScoped<IEmbeddingService, EmbeddingService>();
builder.Services.AddScoped<ISearchService, SearchService>();
builder.Services.AddScoped<IAiQueryService, AiQueryService>();

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
Log.Information("AiAssistantService starting on {Environment}", app.Environment.EnvironmentName);

try
{
    app.Run();
}
catch (Exception ex)
{
    Log.Fatal(ex, "AiAssistantService terminated unexpectedly");
}
finally
{
    Log.CloseAndFlush();
}
