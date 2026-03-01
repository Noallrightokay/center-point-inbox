using Microsoft.EntityFrameworkCore;

namespace EmailService.Models;

public class EmailDbContext : DbContext
{
    public EmailDbContext(DbContextOptions<EmailDbContext> options) : base(options) { }

    public DbSet<EmailAccount> EmailAccounts => Set<EmailAccount>();
    public DbSet<EmailMessage> EmailMessages => Set<EmailMessage>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<EmailAccount>(entity =>
        {
            entity.ToTable("email_accounts");
            entity.HasKey(e => e.Id);
            entity.Property(e => e.Id).HasDefaultValueSql("gen_random_uuid()");
            entity.HasIndex(e => new { e.TenantId, e.Email }).IsUnique();
            entity.Property(e => e.PasswordEncrypted).IsRequired();
        });

        modelBuilder.Entity<EmailMessage>(entity =>
        {
            entity.ToTable("email_messages");
            entity.HasKey(e => e.Id);
            entity.Property(e => e.Id).HasDefaultValueSql("gen_random_uuid()");
            entity.HasIndex(e => new { e.AccountId, e.MessageUid }).IsUnique();
            entity.HasIndex(e => new { e.TenantId, e.ReceivedAt });
        });
    }
}
