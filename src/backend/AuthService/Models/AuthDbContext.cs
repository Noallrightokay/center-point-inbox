using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Storage.ValueConversion;

namespace CenterPointInbox.AuthService.Models;

/// <summary>
/// Entity Framework Core DbContext for the Auth Service database.
/// </summary>
public class AuthDbContext : DbContext
{
    public AuthDbContext(DbContextOptions<AuthDbContext> options) : base(options)
    {
    }

    public DbSet<User> Users => Set<User>();
    public DbSet<OAuthConnection> OAuthConnections => Set<OAuthConnection>();
    public DbSet<Tenant> Tenants => Set<Tenant>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        base.OnModelCreating(modelBuilder);

        ConfigureTenant(modelBuilder);
        ConfigureUser(modelBuilder);
        ConfigureOAuthConnection(modelBuilder);
    }

    private static void ConfigureTenant(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<Tenant>(entity =>
        {
            entity.ToTable("tenants");

            entity.HasKey(t => t.Id);

            entity.Property(t => t.Id)
                .HasColumnName("id")
                .HasDefaultValueSql("gen_random_uuid()");

            entity.Property(t => t.Name)
                .HasColumnName("name")
                .HasMaxLength(256)
                .IsRequired();

            entity.Property(t => t.Domain)
                .HasColumnName("domain")
                .HasMaxLength(256);

            entity.Property(t => t.IsActive)
                .HasColumnName("is_active")
                .HasDefaultValue(true);

            entity.Property(t => t.CreatedAt)
                .HasColumnName("created_at")
                .HasDefaultValueSql("now()");

            entity.Property(t => t.UpdatedAt)
                .HasColumnName("updated_at")
                .HasDefaultValueSql("now()");

            entity.HasIndex(t => t.Domain)
                .IsUnique()
                .HasFilter("domain IS NOT NULL");
        });
    }

    private static void ConfigureUser(ModelBuilder modelBuilder)
    {
        var roleConverter = new ValueConverter<Role, string>(
            v => v.ToString(),
            v => Enum.Parse<Role>(v, ignoreCase: true));

        modelBuilder.Entity<User>(entity =>
        {
            entity.ToTable("users");

            entity.HasKey(u => u.Id);

            entity.Property(u => u.Id)
                .HasColumnName("id")
                .HasDefaultValueSql("gen_random_uuid()");

            entity.Property(u => u.TenantId)
                .HasColumnName("tenant_id")
                .IsRequired();

            entity.Property(u => u.Email)
                .HasColumnName("email")
                .HasMaxLength(320)
                .IsRequired();

            entity.Property(u => u.DisplayName)
                .HasColumnName("display_name")
                .HasMaxLength(256)
                .IsRequired();

            entity.Property(u => u.Role)
                .HasColumnName("role")
                .HasMaxLength(50)
                .HasConversion(roleConverter)
                .IsRequired();

            entity.Property(u => u.RefreshTokenHash)
                .HasColumnName("refresh_token_hash")
                .HasMaxLength(512);

            entity.Property(u => u.RefreshTokenExpiresAt)
                .HasColumnName("refresh_token_expires_at");

            entity.Property(u => u.IsActive)
                .HasColumnName("is_active")
                .HasDefaultValue(true);

            entity.Property(u => u.CreatedAt)
                .HasColumnName("created_at")
                .HasDefaultValueSql("now()");

            entity.Property(u => u.UpdatedAt)
                .HasColumnName("updated_at")
                .HasDefaultValueSql("now()");

            entity.Property(u => u.LastLoginAt)
                .HasColumnName("last_login_at");

            entity.HasIndex(u => new { u.TenantId, u.Email })
                .IsUnique();

            entity.HasIndex(u => u.Email);

            entity.HasOne(u => u.Tenant)
                .WithMany(t => t.Users)
                .HasForeignKey(u => u.TenantId)
                .OnDelete(DeleteBehavior.Restrict);
        });
    }

    private static void ConfigureOAuthConnection(ModelBuilder modelBuilder)
    {
        var providerConverter = new ValueConverter<Provider, string>(
            v => v.ToString(),
            v => Enum.Parse<Provider>(v, ignoreCase: true));

        modelBuilder.Entity<OAuthConnection>(entity =>
        {
            entity.ToTable("oauth_connections");

            entity.HasKey(o => o.Id);

            entity.Property(o => o.Id)
                .HasColumnName("id")
                .HasDefaultValueSql("gen_random_uuid()");

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
                .HasColumnName("created_at")
                .HasDefaultValueSql("now()");

            entity.Property(o => o.UpdatedAt)
                .HasColumnName("updated_at")
                .HasDefaultValueSql("now()");

            entity.HasIndex(o => new { o.UserId, o.Provider })
                .IsUnique();

            entity.HasIndex(o => new { o.Provider, o.ProviderUserId })
                .IsUnique();

            entity.HasOne(o => o.User)
                .WithMany(u => u.OAuthConnections)
                .HasForeignKey(o => o.UserId)
                .OnDelete(DeleteBehavior.Cascade);
        });
    }
}
