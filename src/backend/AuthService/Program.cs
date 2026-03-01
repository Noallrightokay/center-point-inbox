using CenterPointInbox.AuthService.Configuration;
using CenterPointInbox.AuthService.Models;
using CenterPointInbox.AuthService.Services;
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
        .Enrich.WithProperty("ServiceName", "AuthService")
        .WriteTo.Console(
            outputTemplate: "[{Timestamp:HH:mm:ss} {Level:u3}] [{CorrelationId}] {Message:lj}{NewLine}{Exception}");
});

// ---------------------------------------------------------------------------
// Configuration bindings
// ---------------------------------------------------------------------------
builder.Services.Configure<AuthSettings>(
    builder.Configuration.GetSection(AuthSettings.SectionName));

builder.Services.Configure<JwtConfig>(
    builder.Configuration.GetSection(JwtConfig.SectionName));

// ---------------------------------------------------------------------------
// Database context (PostgreSQL via Npgsql)
// ---------------------------------------------------------------------------
builder.Services.AddDbContext<AuthDbContext>(options =>
{
    options.UseNpgsql(
        builder.Configuration.GetConnectionString("AuthDb"),
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
// HTTP client for OAuth provider calls
// ---------------------------------------------------------------------------
builder.Services.AddHttpClient("OAuth", client =>
{
    client.Timeout = TimeSpan.FromSeconds(30);
    client.DefaultRequestHeaders.Accept.Add(
        new System.Net.Http.Headers.MediaTypeWithQualityHeaderValue("application/json"));
});

// ---------------------------------------------------------------------------
// Application service registrations
// ---------------------------------------------------------------------------
builder.Services.AddScoped<IAuthService, AuthServiceImpl>();
builder.Services.AddScoped<ITokenService, TokenService>();
builder.Services.AddScoped<IOAuthService, OAuthService>();

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
Log.Information("AuthService starting on {Environment}", app.Environment.EnvironmentName);

try
{
    app.Run();
}
catch (Exception ex)
{
    Log.Fatal(ex, "AuthService terminated unexpectedly");
}
finally
{
    Log.CloseAndFlush();
}
