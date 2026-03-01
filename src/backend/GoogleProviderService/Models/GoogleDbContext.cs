using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Storage.ValueConversion;

namespace CenterPointInbox.GoogleProviderService.Models;

/// <summary>
/// Minimal read-oriented DbContext for the Google Provider Service.
/// Provides access to OAuthConnections (for reading encrypted tokens)
/// and FilesMetadata (for syncing file metadata from Google Drive).
/// </summary>
public class GoogleDbContext : DbContext
{
    public GoogleDbContext(DbContextOptions<GoogleDbContext> options) : base(options)
    {
    }

    public DbSet<OAuthConnection> OAuthConnections => Set<OAuthConnection>();
    public DbSet<FileMetadata> FilesMetadata => Set<FileMetadata>();

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

        modelBuilder.Entity<OAuthConnection>(entity =>
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
                .HasMaxLength(2048)
                .HasDefaultValue(string.Empty);

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
        modelBuilder.Entity<FileMetadata>(entity =>
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
                .HasMaxLength(512)
                .IsRequired();

            entity.Property(f => f.FileName)
                .HasColumnName("file_name")
                .HasMaxLength(1024)
                .IsRequired();

            entity.Property(f => f.MimeType)
                .HasColumnName("mime_type")
                .HasMaxLength(256)
                .IsRequired();

            entity.Property(f => f.FileSize)
                .HasColumnName("file_size");

            entity.Property(f => f.ProviderModifiedAt)
                .HasColumnName("provider_modified_at");

            entity.Property(f => f.WebViewLink)
                .HasColumnName("web_view_link")
                .HasMaxLength(2048);

            entity.Property(f => f.ThumbnailLink)
                .HasColumnName("thumbnail_link")
                .HasMaxLength(2048);

            entity.Property(f => f.ParentId)
                .HasColumnName("parent_id")
                .HasMaxLength(512);

            entity.Property(f => f.IsShared)
                .HasColumnName("is_shared")
                .HasDefaultValue(false);

            entity.Property(f => f.Owners)
                .HasColumnName("owners")
                .HasMaxLength(4096);

            entity.Property(f => f.SyncedAt)
                .HasColumnName("synced_at")
                .HasDefaultValueSql("now()");

            entity.Property(f => f.CreatedAt)
                .HasColumnName("created_at")
                .HasDefaultValueSql("now()");

            entity.Property(f => f.UpdatedAt)
                .HasColumnName("updated_at")
                .HasDefaultValueSql("now()");

            entity.HasIndex(f => new { f.UserId, f.Provider, f.ProviderFileId })
                .IsUnique();

            entity.HasIndex(f => f.UserId);
        });
    }
}

/// <summary>
/// Represents an OAuth provider connection. Read-only from this service's perspective;
/// the Auth Service is responsible for creating and updating these records.
/// </summary>
public class OAuthConnection
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
/// Represents cached file metadata synced from a cloud storage provider.
/// Used for local searching and indexing without repeated API calls.
/// </summary>
public class FileMetadata
{
    public Guid Id { get; set; }
    public Guid UserId { get; set; }
    public string Provider { get; set; } = string.Empty;
    public string ProviderFileId { get; set; } = string.Empty;
    public string FileName { get; set; } = string.Empty;
    public string MimeType { get; set; } = string.Empty;
    public long? FileSize { get; set; }
    public DateTimeOffset? ProviderModifiedAt { get; set; }
    public string? WebViewLink { get; set; }
    public string? ThumbnailLink { get; set; }
    public string? ParentId { get; set; }
    public bool IsShared { get; set; }
    public string? Owners { get; set; }
    public DateTimeOffset SyncedAt { get; set; } = DateTimeOffset.UtcNow;
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;
}

/// <summary>
/// Supported OAuth providers (mirrors the Auth Service enum for DB mapping).
/// </summary>
public enum OAuthProvider
{
    Google = 0,
    Microsoft = 1
}
