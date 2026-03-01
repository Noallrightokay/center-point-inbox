using Amazon.S3;
using CenterPointInbox.Shared.Extensions;
using CenterPointInbox.TranslationWorker.Configuration;
using CenterPointInbox.TranslationWorker.Models;
using CenterPointInbox.TranslationWorker.Services;
using Microsoft.EntityFrameworkCore;
using Serilog;

var builder = Host.CreateDefaultBuilder(args);

// ---------------------------------------------------------------------------
// Serilog configuration
// ---------------------------------------------------------------------------
builder.UseSerilog((context, loggerConfig) =>
{
    loggerConfig
        .ReadFrom.Configuration(context.Configuration)
        .Enrich.FromLogContext()
        .Enrich.WithProperty("ServiceName", "TranslationWorker")
        .WriteTo.Console(
            outputTemplate: "[{Timestamp:HH:mm:ss} {Level:u3}] [{CorrelationId}] {Message:lj}{NewLine}{Exception}");
});

builder.ConfigureServices((context, services) =>
{
    var configuration = context.Configuration;

    // ---------------------------------------------------------------------------
    // Configuration bindings
    // ---------------------------------------------------------------------------
    services.Configure<TranslationSettings>(
        configuration.GetSection(TranslationSettings.SectionName));

    var translationSettings = configuration
        .GetSection(TranslationSettings.SectionName)
        .Get<TranslationSettings>() ?? new TranslationSettings();

    // ---------------------------------------------------------------------------
    // Database context (PostgreSQL via Npgsql)
    // ---------------------------------------------------------------------------
    services.AddDbContext<TranslationDbContext>(options =>
    {
        options.UseNpgsql(
            configuration.GetConnectionString("TranslationDb"),
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
    // MassTransit with RabbitMQ (register TranslationRequestedConsumer)
    // ---------------------------------------------------------------------------
    services.AddSharedMessaging(configuration, busConfig =>
    {
        busConfig.AddConsumer<TranslationRequestedConsumer>();
    });

    // ---------------------------------------------------------------------------
    // AWS S3 client (MinIO compatible)
    // ---------------------------------------------------------------------------
    services.AddSingleton<IAmazonS3>(_ =>
    {
        var s3Config = new AmazonS3Config
        {
            ServiceURL = translationSettings.S3.Endpoint,
            ForcePathStyle = translationSettings.S3.ForcePathStyle,
            AuthenticationRegion = translationSettings.S3.Region
        };

        return new AmazonS3Client(
            translationSettings.S3.AccessKey,
            translationSettings.S3.SecretKey,
            s3Config);
    });

    // ---------------------------------------------------------------------------
    // HTTP client for downloading files from provider services
    // ---------------------------------------------------------------------------
    services.AddHttpClient("ProviderService", client =>
    {
        client.Timeout = TimeSpan.FromSeconds(translationSettings.DownloadTimeoutSeconds);
        client.DefaultRequestHeaders.Accept.Add(
            new System.Net.Http.Headers.MediaTypeWithQualityHeaderValue("application/octet-stream"));
    });

    // ---------------------------------------------------------------------------
    // Application service registrations
    // ---------------------------------------------------------------------------
    services.AddScoped<IDocumentConverter, DocumentConverter>();
    services.AddScoped<IStorageService, StorageService>();
    services.AddScoped<IPhiRedactor, PhiRedactor>();
});

// ---------------------------------------------------------------------------
// Build and run the worker host
// ---------------------------------------------------------------------------
var host = builder.Build();

Log.Information("TranslationWorker starting on {Environment}",
    host.Services.GetRequiredService<IHostEnvironment>().EnvironmentName);

try
{
    await host.RunAsync();
}
catch (Exception ex)
{
    Log.Fatal(ex, "TranslationWorker terminated unexpectedly");
}
finally
{
    Log.CloseAndFlush();
}
