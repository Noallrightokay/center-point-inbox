using Microsoft.EntityFrameworkCore;

namespace CenterPointInbox.TranslationWorker.Models;

/// <summary>
/// Entity Framework DbContext for the Translation Worker.
/// Provides access to translation jobs and file metadata tables.
/// </summary>
public class TranslationDbContext : DbContext
{
    public TranslationDbContext(DbContextOptions<TranslationDbContext> options)
        : base(options)
    {
    }

    public DbSet<TranslationJob> TranslationJobs => Set<TranslationJob>();
    public DbSet<FileMetadata> FilesMetadata => Set<FileMetadata>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        base.OnModelCreating(modelBuilder);

        modelBuilder.Entity<TranslationJob>(entity =>
        {
            entity.ToTable("translation_jobs");

            entity.HasKey(e => e.Id);

            entity.Property(e => e.Status)
                .HasConversion<string>()
                .HasMaxLength(50);

            entity.HasIndex(e => e.TenantId)
                .HasDatabaseName("ix_translation_jobs_tenant_id");

            entity.HasIndex(e => e.UserId)
                .HasDatabaseName("ix_translation_jobs_user_id");

            entity.HasIndex(e => e.SourceFileId)
                .HasDatabaseName("ix_translation_jobs_source_file_id");

            entity.HasIndex(e => e.Status)
                .HasDatabaseName("ix_translation_jobs_status");

            entity.HasIndex(e => e.CreatedAt)
                .HasDatabaseName("ix_translation_jobs_created_at");
        });

        modelBuilder.Entity<FileMetadata>(entity =>
        {
            entity.ToTable("files_metadata");

            entity.HasKey(e => e.Id);

            entity.HasIndex(e => e.TenantId)
                .HasDatabaseName("ix_files_metadata_tenant_id");

            entity.HasIndex(e => e.UserId)
                .HasDatabaseName("ix_files_metadata_user_id");

            entity.HasIndex(e => new { e.TenantId, e.Provider })
                .HasDatabaseName("ix_files_metadata_tenant_provider");
        });
    }
}
