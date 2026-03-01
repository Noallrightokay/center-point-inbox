using Microsoft.EntityFrameworkCore;

namespace CenterPointInbox.AiAssistantService.Models;

/// <summary>
/// Entity Framework DbContext for the AI Assistant Service.
/// Manages embeddings and file metadata tables with pgvector support
/// for semantic vector search operations.
/// </summary>
public class AiDbContext : DbContext
{
    public AiDbContext(DbContextOptions<AiDbContext> options) : base(options)
    {
    }

    public DbSet<Embedding> Embeddings => Set<Embedding>();
    public DbSet<FileMetadata> FilesMetadata => Set<FileMetadata>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        base.OnModelCreating(modelBuilder);

        // Enable the pgvector extension
        modelBuilder.HasPostgresExtension("vector");

        // ─────────────────────────────────────────────────────────────────────
        // Embedding entity configuration
        // ─────────────────────────────────────────────────────────────────────
        modelBuilder.Entity<Embedding>(entity =>
        {
            entity.ToTable("embeddings");

            entity.HasKey(e => e.Id);

            entity.Property(e => e.Id)
                .HasColumnName("id")
                .ValueGeneratedOnAdd();

            entity.Property(e => e.TenantId)
                .HasColumnName("tenant_id")
                .IsRequired();

            entity.Property(e => e.FileId)
                .HasColumnName("file_id")
                .IsRequired();

            entity.Property(e => e.ChunkIndex)
                .HasColumnName("chunk_index")
                .IsRequired();

            entity.Property(e => e.ChunkText)
                .HasColumnName("chunk_text")
                .IsRequired();

            entity.Property(e => e.EmbeddingVector)
                .HasColumnName("embedding")
                .HasColumnType("vector(1536)")
                .IsRequired();

            entity.Property(e => e.Metadata)
                .HasColumnName("metadata")
                .HasColumnType("jsonb");

            entity.Property(e => e.CreatedAt)
                .HasColumnName("created_at")
                .IsRequired();

            // Indexes for efficient querying
            entity.HasIndex(e => e.TenantId)
                .HasDatabaseName("ix_embeddings_tenant_id");

            entity.HasIndex(e => e.FileId)
                .HasDatabaseName("ix_embeddings_file_id");

            entity.HasIndex(e => new { e.TenantId, e.FileId })
                .HasDatabaseName("ix_embeddings_tenant_file");
        });

        // ─────────────────────────────────────────────────────────────────────
        // FileMetadata entity configuration (read-only view for ACL checks)
        // ─────────────────────────────────────────────────────────────────────
        modelBuilder.Entity<FileMetadata>(entity =>
        {
            entity.ToTable("files_metadata");

            entity.HasKey(e => e.Id);

            entity.Property(e => e.Id)
                .HasColumnName("id");

            entity.Property(e => e.UserId)
                .HasColumnName("user_id")
                .IsRequired();

            entity.Property(e => e.TenantId)
                .HasColumnName("tenant_id")
                .IsRequired();

            entity.Property(e => e.Provider)
                .HasColumnName("provider")
                .IsRequired();

            entity.Property(e => e.FileName)
                .HasColumnName("file_name")
                .IsRequired();

            entity.Property(e => e.Quarantined)
                .HasColumnName("quarantined")
                .HasDefaultValue(false);

            entity.Property(e => e.IsShared)
                .HasColumnName("is_shared")
                .HasDefaultValue(false);
        });
    }
}
