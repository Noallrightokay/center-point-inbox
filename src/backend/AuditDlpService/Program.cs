using CenterPointInbox.AuditDlpService.Models;
using CenterPointInbox.AuditDlpService.Services;
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
        .Enrich.WithProperty("ServiceName", "AuditDlpService")
        .WriteTo.Console(
            outputTemplate: "[{Timestamp:HH:mm:ss} {Level:u3}] [{CorrelationId}] {Message:lj}{NewLine}{Exception}");
});

// ---------------------------------------------------------------------------
// Database context (PostgreSQL via Npgsql)
// ---------------------------------------------------------------------------
builder.Services.AddDbContext<AuditDlpDbContext>(options =>
{
    options.UseNpgsql(
        builder.Configuration.GetConnectionString("AuditDlpDb"),
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

// Configure MassTransit with AuditDlpService-specific consumers
builder.Services.AddSharedMessaging(builder.Configuration, busConfig =>
{
    busConfig.AddConsumer<AuditEventConsumer>();
    busConfig.AddConsumer<DlpScanRequestedConsumer>();
});

// ---------------------------------------------------------------------------
// Application service registrations
// ---------------------------------------------------------------------------
builder.Services.AddScoped<IAuditService, AuditService>();
builder.Services.AddScoped<IDlpService, DlpService>();

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
Log.Information("AuditDlpService starting on {Environment}", app.Environment.EnvironmentName);

try
{
    app.Run();
}
catch (Exception ex)
{
    Log.Fatal(ex, "AuditDlpService terminated unexpectedly");
}
finally
{
    Log.CloseAndFlush();
}
