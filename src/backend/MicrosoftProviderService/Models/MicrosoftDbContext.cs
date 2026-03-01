using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Storage.ValueConversion;

namespace CenterPointInbox.MicrosoftProviderService.Models;

/// <summary>
/// Minimal Entity Framework Core DbContext for the Microsoft Provider Service.
/// Provides read access to OAuthConnections and read/write access to FilesMetadata.
/// Shares the same database as the Auth Service but only maps the tables it needs.
/// </summary>
public class MicrosoftDbContext : DbContext
{
    public MicrosoftDbContext(DbContextOptions<MicrosoftDbContext> options) : base(options)
    {
    }

    public DbSet<OAuthConnectionEntity> OAuthConnections => Set<OAuthConnectionEntity>();
    public DbSet<FileMetadataEntity> FilesMetadata => Set<FileMetadataEntity>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        base.OnModelCreating(modelBuilder);

        ConfigureOAuthConnection(modelBuilder);
        ConfigureFileMetadata(modelBuilder);
    }

    private static void ConfigureOAuthConnection(ModelBuilder modelBuilder)
    {
        var providerConverter = new ValueConverter<OAuthProvider, string>(
            v => v.ToString(),
            v => Enum.Parse<OAuthProvider>(v, ignoreCase: true));

        modelBuilder.Entity<OAuthConnectionEntity>(entity =>
        {
            entity.ToTable("oauth_connections");

            entity.HasKey(o => o.Id);

            entity.Property(o => o.Id)
                .HasColumnName("id");

            entity.Property(o => o.UserId)
                .HasColumnName("user_id")
                .IsRequired();

            entity.Property(o => o.Provider)
                .HasColumnName("provider")
                .HasMaxLength(50)
                .HasConversion(providerConverter)
                .IsRequired();

            entity.Property(o => o.ProviderUserId)
                .HasColumnName("provider_user_id")
                .HasMaxLength(256)
                .IsRequired();

            entity.Property(o => o.ProviderEmail)
                .HasColumnName("provider_email")
                .HasMaxLength(320)
                .IsRequired();

            entity.Property(o => o.EncryptedAccessToken)
                .HasColumnName("encrypted_access_token")
                .IsRequired();

            entity.Property(o => o.EncryptedRefreshToken)
                .HasColumnName("encrypted_refresh_token");

            entity.Property(o => o.AccessTokenExpiresAt)
                .HasColumnName("access_token_expires_at");

            entity.Property(o => o.Scopes)
                .HasColumnName("scopes")
                .HasMaxLength(2048);

            entity.Property(o => o.CreatedAt)
                .HasColumnName("created_at");

            entity.Property(o => o.UpdatedAt)
                .HasColumnName("updated_at");

            entity.HasIndex(o => new { o.UserId, o.Provider })
                .IsUnique();
        });
    }

    private static void ConfigureFileMetadata(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<FileMetadataEntity>(entity =>
        {
            entity.ToTable("files_metadata");

            entity.HasKey(f => f.Id);

            entity.Property(f => f.Id)
                .HasColumnName("id")
                .HasDefaultValueSql("gen_random_uuid()");

            entity.Property(f => f.UserId)
                .HasColumnName("user_id")
                .IsRequired();

            entity.Property(f => f.Provider)
                .HasColumnName("provider")
                .HasMaxLength(50)
                .IsRequired();

            entity.Property(f => f.ProviderFileId)
                .HasColumnName("provider_file_id")
                .HasMaxLength(1024)
                .IsRequired();

            entity.Property(f => f.FileName)
                .HasColumnName("file_name")
                .HasMaxLength(512)
                .IsRequired();

            entity.Property(f => f.MimeType)
                .HasColumnName("mime_type")
                .HasMaxLength(256);

            entity.Property(f => f.Size)
                .HasColumnName("size");

            entity.Property(f => f.WebUrl)
                .HasColumnName("web_url")
                .HasMaxLength(2048);

            entity.Property(f => f.ParentPath)
                .HasColumnName("parent_path")
                .HasMaxLength(2048);

            entity.Property(f => f.IsShared)
                .HasColumnName("is_shared")
                .HasDefaultValue(false);

            entity.Property(f => f.LastModifiedAt)
                .HasColumnName("last_modified_at");

            entity.Property(f => f.SyncedAt)
                .HasColumnName("synced_at")
                .HasDefaultValueSql("now()");

            entity.Property(f => f.CreatedAt)
                .HasColumnName("created_at")
                .HasDefaultValueSql("now()");

            entity.HasIndex(f => new { f.UserId, f.Provider, f.ProviderFileId })
                .IsUnique();

            entity.HasIndex(f => f.UserId);
        });
    }
}

/// <summary>
/// Lightweight entity representing an OAuth connection record.
/// Mirrors the auth service's oauth_connections table (read-only usage here).
/// </summary>
public class OAuthConnectionEntity
{
    public Guid Id { get; set; }
    public Guid UserId { get; set; }
    public OAuthProvider Provider { get; set; }
    public string ProviderUserId { get; set; } = string.Empty;
    public string ProviderEmail { get; set; } = string.Empty;
    public string EncryptedAccessToken { get; set; } = string.Empty;
    public string? EncryptedRefreshToken { get; set; }
    public DateTimeOffset? AccessTokenExpiresAt { get; set; }
    public string Scopes { get; set; } = string.Empty;
    public DateTimeOffset CreatedAt { get; set; }
    public DateTimeOffset UpdatedAt { get; set; }
}

/// <summary>
/// Entity representing synced file metadata from a cloud provider.
/// </summary>
public class FileMetadataEntity
{
    public Guid Id { get; set; }
    public Guid UserId { get; set; }
    public string Provider { get; set; } = string.Empty;
    public string ProviderFileId { get; set; } = string.Empty;
    public string FileName { get; set; } = string.Empty;
    public string? MimeType { get; set; }
    public long? Size { get; set; }
    public string? WebUrl { get; set; }
    public string? ParentPath { get; set; }
    public bool IsShared { get; set; }
    public DateTimeOffset? LastModifiedAt { get; set; }
    public DateTimeOffset SyncedAt { get; set; } = DateTimeOffset.UtcNow;
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
}

/// <summary>
/// Supported OAuth providers (matches the auth service enum values).
/// </summary>
public enum OAuthProvider
{
    Google = 0,
    Microsoft = 1
}
