using Microsoft.EntityFrameworkCore;
using EmailService.Configuration;
using EmailService.Models;
using EmailService.Services;
using Shared.Extensions;

var builder = WebApplication.CreateBuilder(args);

builder.Services.AddControllers();
builder.Services.AddEndpointsApiExplorer();

builder.Services.AddDbContext<EmailDbContext>(options =>
    options.UseNpgsql(builder.Configuration.GetConnectionString("DefaultConnection")));

builder.Services.Configure<EmailSettings>(builder.Configuration.GetSection("Email"));

builder.Services.AddScoped<IImapService, ImapService>();
builder.Services.AddScoped<ISmtpService, SmtpService>();

builder.Services.AddSharedAuthentication(builder.Configuration);
builder.Services.AddSharedCors();

builder.Services.AddHealthChecks();

var app = builder.Build();

app.UseSharedMiddleware();
app.UseAuthentication();
app.UseAuthorization();
app.MapControllers();
app.MapHealthChecks("/health/live");

app.Run();
