using System.Text.RegularExpressions;
using CenterPointInbox.AuditDlpService.Models;
using CenterPointInbox.Shared.Contracts;
using MassTransit;
using Microsoft.EntityFrameworkCore;

namespace CenterPointInbox.AuditDlpService.Services;

/// <summary>
/// MassTransit consumer that processes <see cref="DlpScanRequested"/> messages
/// by scanning content for PHI (Protected Health Information) patterns.
/// When violations are detected, it creates DlpViolation records, quarantines
/// the file, and publishes notification events.
/// </summary>
public class DlpScanRequestedConsumer : IConsumer<DlpScanRequested>
{
    private readonly AuditDlpDbContext _dbContext;
    private readonly IPublishEndpoint _publishEndpoint;
    private readonly ILogger<DlpScanRequestedConsumer> _logger;

    /// <summary>
    /// PHI pattern definitions used for content scanning.
    /// Each entry maps a violation type to its regex pattern, severity, and description.
    /// </summary>
    private static readonly List<PhiPattern> PhiPatterns =
    [
        new PhiPattern
        {
            ViolationType = "SSN",
            Pattern = @"\b\d{3}-\d{2}-\d{4}\b",
            Severity = Severity.Critical,
            Description = "Social Security Number detected"
        },
        new PhiPattern
        {
            ViolationType = "SSN_UNFORMATTED",
            Pattern = @"\b(?!000|666|9\d{2})\d{3}(?!00)\d{2}(?!0000)\d{4}\b",
            Severity = Severity.High,
            Description = "Potential unformatted Social Security Number detected"
        },
        new PhiPattern
        {
            ViolationType = "PHONE",
            Pattern = @"\b(?:\+?1[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b",
            Severity = Severity.Medium,
            Description = "Phone number detected"
        },
        new PhiPattern
        {
            ViolationType = "EMAIL",
            Pattern = @"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b",
            Severity = Severity.Low,
            Description = "Email address detected"
        },
        new PhiPattern
        {
            ViolationType = "MRN",
            Pattern = @"\b(?:MRN|Medical Record Number|Med Rec)[:\s#]*\d{6,12}\b",
            Severity = Severity.Critical,
            Description = "Medical Record Number (MRN) detected"
        },
        new PhiPattern
        {
            ViolationType = "DOB",
            Pattern = @"\b(?:DOB|Date of Birth|Birth Date)[:\s]*(?:\d{1,2}[/-]\d{1,2}[/-]\d{2,4}|\d{4}[/-]\d{1,2}[/-]\d{1,2})\b",
            Severity = Severity.High,
            Description = "Date of Birth detected"
        },
        new PhiPattern
        {
            ViolationType = "PATIENT_NAME",
            Pattern = @"\b(?:Patient|Pt\.?\s*Name|Patient\s*Name)[:\s]+[A-Z][a-zA-Z'-]+(?:\s+[A-Z][a-zA-Z'-]+){1,3}\b",
            Severity = Severity.High,
            Description = "Patient name pattern detected"
        },
        new PhiPattern
        {
            ViolationType = "HEALTH_PLAN_ID",
            Pattern = @"\b(?:Health Plan|Insurance|Policy)[:\s#]*(?:[A-Z]{2,4})?\d{8,15}\b",
            Severity = Severity.High,
            Description = "Health plan or insurance ID detected"
        }
    ];

    public DlpScanRequestedConsumer(
        AuditDlpDbContext dbContext,
        IPublishEndpoint publishEndpoint,
        ILogger<DlpScanRequestedConsumer> logger)
    {
        _dbContext = dbContext;
        _publishEndpoint = publishEndpoint;
        _logger = logger;
    }

    public async Task Consume(ConsumeContext<DlpScanRequested> context)
    {
        var request = context.Message;

        _logger.LogInformation(
            "Starting DLP scan for file {FileId} in tenant {TenantId}",
            request.FileId, request.TenantId);

        if (string.IsNullOrWhiteSpace(request.Content))
        {
            _logger.LogDebug("Skipping DLP scan for file {FileId}: content is empty", request.FileId);
            return;
        }

        var violations = ScanContent(request.TenantId, request.FileId, request.Content);

        if (violations.Count == 0)
        {
            _logger.LogInformation(
                "DLP scan complete for file {FileId}: no violations detected",
                request.FileId);
            return;
        }

        _logger.LogWarning(
            "DLP scan for file {FileId} detected {Count} violation(s)",
            request.FileId, violations.Count);

        // Persist all violation records
        _dbContext.DlpViolations.AddRange(violations);

        // Quarantine the file
        var file = await _dbContext.FilesMetadata
            .Where(f => f.TenantId == request.TenantId && f.Id == request.FileId)
            .FirstOrDefaultAsync();

        if (file is not null)
        {
            file.IsQuarantined = true;
            file.QuarantineReason = $"DLP scan detected {violations.Count} PHI violation(s): " +
                string.Join(", ", violations.Select(v => v.ViolationType).Distinct());
            file.UpdatedAt = DateTimeOffset.UtcNow;

            _logger.LogInformation(
                "File {FileId} quarantined due to {Count} DLP violation(s)",
                request.FileId, violations.Count);
        }
        else
        {
            _logger.LogWarning(
                "File metadata not found for file {FileId} in tenant {TenantId}; unable to quarantine",
                request.FileId, request.TenantId);
        }

        await _dbContext.SaveChangesAsync();

        // Publish DlpViolationDetected events for each violation
        var publishTasks = violations.Select(violation =>
            _publishEndpoint.Publish(new DlpViolationDetected
            {
                TenantId = request.TenantId,
                FileId = request.FileId,
                ViolationType = violation.ViolationType,
                Severity = MapToContractSeverity(violation.Severity),
                Pattern = violation.PatternMatched,
                DetectedAt = DateTimeOffset.UtcNow
            }));

        await Task.WhenAll(publishTasks);

        // Publish a single audit event summarizing the DLP scan results
        await _publishEndpoint.Publish(new AuditEvent
        {
            TenantId = request.TenantId,
            UserId = Guid.Empty, // system action
            Action = "dlp.scan.violations_detected",
            ResourceType = "file",
            ResourceId = request.FileId.ToString(),
            Metadata = new Dictionary<string, string>
            {
                ["violationCount"] = violations.Count.ToString(),
                ["violationTypes"] = string.Join(", ", violations.Select(v => v.ViolationType).Distinct()),
                ["highestSeverity"] = violations.Max(v => v.Severity).ToString()
            },
            OccurredAt = DateTimeOffset.UtcNow
        });

        _logger.LogInformation(
            "DLP scan complete for file {FileId}: published {Count} violation event(s) and audit event",
            request.FileId, violations.Count);
    }

    /// <summary>
    /// Scans the provided content against all configured PHI patterns
    /// and returns a list of DlpViolation entities for any matches found.
    /// </summary>
    private static List<DlpViolation> ScanContent(Guid tenantId, Guid fileId, string content)
    {
        var violations = new List<DlpViolation>();

        foreach (var phiPattern in PhiPatterns)
        {
            try
            {
                var regex = new Regex(
                    phiPattern.Pattern,
                    RegexOptions.IgnoreCase | RegexOptions.Compiled,
                    TimeSpan.FromSeconds(5));

                var matches = regex.Matches(content);

                if (matches.Count > 0)
                {
                    violations.Add(new DlpViolation
                    {
                        Id = Guid.NewGuid(),
                        TenantId = tenantId,
                        FileId = fileId,
                        ViolationType = phiPattern.ViolationType,
                        Severity = phiPattern.Severity,
                        Description = $"{phiPattern.Description} ({matches.Count} occurrence(s))",
                        PatternMatched = phiPattern.Pattern,
                        Resolved = false,
                        CreatedAt = DateTimeOffset.UtcNow
                    });
                }
            }
            catch (RegexMatchTimeoutException)
            {
                // If a pattern times out, record it as a violation that needs review
                violations.Add(new DlpViolation
                {
                    Id = Guid.NewGuid(),
                    TenantId = tenantId,
                    FileId = fileId,
                    ViolationType = phiPattern.ViolationType,
                    Severity = Severity.High,
                    Description = $"{phiPattern.Description} - scan timed out, manual review required",
                    PatternMatched = phiPattern.Pattern,
                    Resolved = false,
                    CreatedAt = DateTimeOffset.UtcNow
                });
            }
        }

        return violations;
    }

    /// <summary>
    /// Maps the local Severity enum to the shared contract DlpSeverity enum.
    /// </summary>
    private static DlpSeverity MapToContractSeverity(Severity severity)
    {
        return severity switch
        {
            Severity.Low => DlpSeverity.Low,
            Severity.Medium => DlpSeverity.Medium,
            Severity.High => DlpSeverity.High,
            Severity.Critical => DlpSeverity.Critical,
            _ => DlpSeverity.Medium
        };
    }

    /// <summary>
    /// Internal record representing a PHI pattern definition used for DLP scanning.
    /// </summary>
    private sealed class PhiPattern
    {
        public required string ViolationType { get; init; }
        public required string Pattern { get; init; }
        public required Severity Severity { get; init; }
        public required string Description { get; init; }
    }
}
