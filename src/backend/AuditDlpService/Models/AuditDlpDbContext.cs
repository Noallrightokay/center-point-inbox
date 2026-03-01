using System.Text.Json;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.ChangeTracking;
using Microsoft.EntityFrameworkCore.Storage.ValueConversion;

namespace CenterPointInbox.AuditDlpService.Models;

/// <summary>
/// Entity Framework Core DbContext for the Audit and DLP Service database.
/// </summary>
public class AuditDlpDbContext : DbContext
{
    public AuditDlpDbContext(DbContextOptions<AuditDlpDbContext> options) : base(options)
    {
    }

    public DbSet<AuditLog> AuditLogs => Set<AuditLog>();
    public DbSet<DlpViolation> DlpViolations => Set<DlpViolation>();
    public DbSet<FileMetadata> FilesMetadata => Set<FileMetadata>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        base.OnModelCreating(modelBuilder);

        ConfigureAuditLog(modelBuilder);
        ConfigureDlpViolation(modelBuilder);
        ConfigureFileMetadata(modelBuilder);
    }

    private static void ConfigureAuditLog(ModelBuilder modelBuilder)
    {
        var jsonSerializerOptions = new JsonSerializerOptions
        {
            PropertyNamingPolicy = JsonNamingPolicy.CamelCase
        };

        var metadataConverter = new ValueConverter<Dictionary<string, object>, string>(
            v => JsonSerializer.Serialize(v, jsonSerializerOptions),
            v => JsonSerializer.Deserialize<Dictionary<string, object>>(v, jsonSerializerOptions)
                ?? new Dictionary<string, object>());

        var metadataComparer = new ValueComparer<Dictionary<string, object>>(
            (a, b) => JsonSerializer.Serialize(a, jsonSerializerOptions) == JsonSerializer.Serialize(b, jsonSerializerOptions),
            v => JsonSerializer.Serialize(v, jsonSerializerOptions).GetHashCode(),
            v => JsonSerializer.Deserialize<Dictionary<string, object>>(
                JsonSerializer.Serialize(v, jsonSerializerOptions), jsonSerializerOptions)!);

        modelBuilder.Entity<AuditLog>(entity =>
        {
            entity.ToTable("audit_logs");

            entity.HasKey(a => a.Id);

            entity.Property(a => a.Id)
                .HasColumnName("id")
                .HasDefaultValueSql("gen_random_uuid()");

            entity.Property(a => a.TenantId)
                .HasColumnName("tenant_id")
                .IsRequired();

            entity.Property(a => a.UserId)
                .HasColumnName("user_id")
                .IsRequired();

            entity.Property(a => a.Action)
                .HasColumnName("action")
                .HasMaxLength(256)
                .IsRequired();

            entity.Property(a => a.ResourceType)
                .HasColumnName("resource_type")
                .HasMaxLength(128)
                .IsRequired();

            entity.Property(a => a.ResourceId)
                .HasColumnName("resource_id")
                .HasMaxLength(256)
                .IsRequired();

            entity.Property(a => a.IpAddress)
                .HasColumnName("ip_address")
                .HasMaxLength(45);

            entity.Property(a => a.UserAgent)
                .HasColumnName("user_agent")
                .HasMaxLength(1024);

            entity.Property(a => a.Metadata)
                .HasColumnName("metadata")
                .HasColumnType("jsonb")
                .HasConversion(metadataConverter)
                .Metadata.SetValueComparer(metadataComparer);

            entity.Property(a => a.CreatedAt)
                .HasColumnName("created_at")
                .HasDefaultValueSql("now()");

            entity.HasIndex(a => a.TenantId);
            entity.HasIndex(a => a.UserId);
            entity.HasIndex(a => a.Action);
            entity.HasIndex(a => a.CreatedAt);
            entity.HasIndex(a => new { a.TenantId, a.CreatedAt });
        });
    }

    private static void ConfigureDlpViolation(ModelBuilder modelBuilder)
    {
        var severityConverter = new ValueConverter<Severity, string>(
            v => v.ToString(),
            v => Enum.Parse<Severity>(v, ignoreCase: true));

        modelBuilder.Entity<DlpViolation>(entity =>
        {
            entity.ToTable("dlp_violations");

            entity.HasKey(d => d.Id);

            entity.Property(d => d.Id)
                .HasColumnName("id")
                .HasDefaultValueSql("gen_random_uuid()");

            entity.Property(d => d.TenantId)
                .HasColumnName("tenant_id")
                .IsRequired();

            entity.Property(d => d.FileId)
                .HasColumnName("file_id")
                .IsRequired();

            entity.Property(d => d.ViolationType)
                .HasColumnName("violation_type")
                .HasMaxLength(128)
                .IsRequired();

            entity.Property(d => d.Severity)
                .HasColumnName("severity")
                .HasMaxLength(50)
                .HasConversion(severityConverter)
                .IsRequired();

            entity.Property(d => d.Description)
                .HasColumnName("description")
                .HasMaxLength(2048)
                .IsRequired();

            entity.Property(d => d.PatternMatched)
                .HasColumnName("pattern_matched")
                .HasMaxLength(512)
                .IsRequired();

            entity.Property(d => d.Resolved)
                .HasColumnName("resolved")
                .HasDefaultValue(false);

            entity.Property(d => d.ResolvedByUserId)
                .HasColumnName("resolved_by_user_id");

            entity.Property(d => d.ResolvedAt)
                .HasColumnName("resolved_at");

            entity.Property(d => d.CreatedAt)
                .HasColumnName("created_at")
                .HasDefaultValueSql("now()");

            entity.HasIndex(d => d.TenantId);
            entity.HasIndex(d => d.FileId);
            entity.HasIndex(d => d.Severity);
            entity.HasIndex(d => d.Resolved);
            entity.HasIndex(d => new { d.TenantId, d.Resolved });
        });
    }

    private static void ConfigureFileMetadata(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<FileMetadata>(entity =>
        {
            entity.ToTable("files_metadata");

            entity.HasKey(f => f.Id);

            entity.Property(f => f.Id)
                .HasColumnName("id")
                .HasDefaultValueSql("gen_random_uuid()");

            entity.Property(f => f.TenantId)
                .HasColumnName("tenant_id")
                .IsRequired();

            entity.Property(f => f.UserId)
                .HasColumnName("user_id")
                .IsRequired();

            entity.Property(f => f.FileName)
                .HasColumnName("file_name")
                .HasMaxLength(512)
                .IsRequired();

            entity.Property(f => f.ContentType)
                .HasColumnName("content_type")
                .HasMaxLength(256)
                .IsRequired();

            entity.Property(f => f.SizeBytes)
                .HasColumnName("size_bytes")
                .IsRequired();

            entity.Property(f => f.IsQuarantined)
                .HasColumnName("is_quarantined")
                .HasDefaultValue(false);

            entity.Property(f => f.QuarantineReason)
                .HasColumnName("quarantine_reason")
                .HasMaxLength(2048);

            entity.Property(f => f.CreatedAt)
                .HasColumnName("created_at")
                .HasDefaultValueSql("now()");

            entity.Property(f => f.UpdatedAt)
                .HasColumnName("updated_at")
                .HasDefaultValueSql("now()");

            entity.HasIndex(f => f.TenantId);
            entity.HasIndex(f => f.UserId);
            entity.HasIndex(f => f.IsQuarantined);
        });
    }
}
