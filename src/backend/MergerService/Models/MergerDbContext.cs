using Microsoft.EntityFrameworkCore;

namespace CenterPointInbox.MergerService.Models;

/// <summary>
/// DbContext for the Merger Service. Provides read access to files_metadata
/// and read/write access to translation_jobs.
/// </summary>
public class MergerDbContext : DbContext
{
    public MergerDbContext(DbContextOptions<MergerDbContext> options) : base(options)
    {
    }

    public DbSet<FileMetadata> FilesMetadata => Set<FileMetadata>();
    public DbSet<TranslationJob> TranslationJobs => Set<TranslationJob>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        base.OnModelCreating(modelBuilder);

        ConfigureFileMetadata(modelBuilder);
        ConfigureTranslationJob(modelBuilder);
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

            entity.Property(f => f.TenantId)
                .HasColumnName("tenant_id")
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
                .HasMaxLength(1024)
                .IsRequired();

            entity.Property(f => f.MimeType)
                .HasColumnName("mime_type")
                .HasMaxLength(255)
                .IsRequired();

            entity.Property(f => f.FileSizeBytes)
                .HasColumnName("file_size_bytes");

            entity.Property(f => f.ParentFolderId)
                .HasColumnName("parent_folder_id")
                .HasMaxLength(1024);

            entity.Property(f => f.ProviderUrl)
                .HasColumnName("provider_url");

            entity.Property(f => f.LastModifiedProvider)
                .HasColumnName("last_modified_provider");

            entity.Property(f => f.IsShared)
                .HasColumnName("is_shared")
                .HasDefaultValue(false);

            entity.Property(f => f.PermissionsJson)
                .HasColumnName("permissions_json")
                .HasColumnType("jsonb")
                .HasDefaultValueSql("'[]'::jsonb");

            entity.Property(f => f.PhiDetected)
                .HasColumnName("phi_detected")
                .HasDefaultValue(false);

            entity.Property(f => f.Quarantined)
                .HasColumnName("quarantined")
                .HasDefaultValue(false);

            entity.Property(f => f.CreatedAt)
                .HasColumnName("created_at")
                .HasDefaultValueSql("now()");

            entity.Property(f => f.UpdatedAt)
                .HasColumnName("updated_at")
                .HasDefaultValueSql("now()");

            entity.HasIndex(f => new { f.TenantId, f.Provider, f.ProviderFileId })
                .IsUnique()
                .HasDatabaseName("uq_files_tenant_provider_fileid");

            entity.HasIndex(f => f.UserId)
                .HasDatabaseName("idx_files_metadata_user_id");
        });
    }

    private static void ConfigureTranslationJob(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<TranslationJob>(entity =>
        {
            entity.ToTable("translation_jobs");

            entity.HasKey(t => t.Id);

            entity.Property(t => t.Id)
                .HasColumnName("id")
                .HasDefaultValueSql("gen_random_uuid()");

            entity.Property(t => t.UserId)
                .HasColumnName("user_id")
                .IsRequired();

            entity.Property(t => t.TenantId)
                .HasColumnName("tenant_id")
                .IsRequired();

            entity.Property(t => t.SourceFileId)
                .HasColumnName("source_file_id")
                .IsRequired();

            entity.Property(t => t.SourceFormat)
                .HasColumnName("source_format")
                .HasMaxLength(50)
                .IsRequired();

            entity.Property(t => t.TargetFormat)
                .HasColumnName("target_format")
                .HasMaxLength(50)
                .IsRequired();

            entity.Property(t => t.Status)
                .HasColumnName("status")
                .HasMaxLength(20)
                .HasDefaultValue("queued")
                .IsRequired();

            entity.Property(t => t.OutputStoragePath)
                .HasColumnName("output_storage_path");

            entity.Property(t => t.ErrorMessage)
                .HasColumnName("error_message");

            entity.Property(t => t.PhiRedacted)
                .HasColumnName("phi_redacted")
                .HasDefaultValue(false);

            entity.Property(t => t.StartedAt)
                .HasColumnName("started_at");

            entity.Property(t => t.CompletedAt)
                .HasColumnName("completed_at");

            entity.Property(t => t.CreatedAt)
                .HasColumnName("created_at")
                .HasDefaultValueSql("now()");

            entity.Property(t => t.UpdatedAt)
                .HasColumnName("updated_at")
                .HasDefaultValueSql("now()");

            entity.HasOne(t => t.SourceFile)
                .WithMany()
                .HasForeignKey(t => t.SourceFileId)
                .OnDelete(DeleteBehavior.Cascade);

            entity.HasIndex(t => t.TenantId)
                .HasDatabaseName("idx_translation_jobs_tenant_id");

            entity.HasIndex(t => t.UserId)
                .HasDatabaseName("idx_translation_jobs_user_id");

            entity.HasIndex(t => t.SourceFileId)
                .HasDatabaseName("idx_translation_jobs_source_file");
        });
    }
}
