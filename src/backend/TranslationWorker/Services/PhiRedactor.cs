using System.Text.RegularExpressions;
using Microsoft.Extensions.Logging;

namespace CenterPointInbox.TranslationWorker.Services;

/// <summary>
/// Pattern-based PHI redaction service.
/// Detects and redacts the following PHI types:
///   - Social Security Numbers (SSN)
///   - Phone numbers (various US formats)
///   - Email addresses
///   - Medical Record Numbers (MRN)
///   - Dates of birth
///   - Patient/person names following "Patient:" or "Name:" labels
/// </summary>
public class PhiRedactor : IPhiRedactor
{
    private readonly ILogger<PhiRedactor> _logger;

    /// <summary>
    /// Ordered list of PHI detection patterns. Each entry maps a pattern category
    /// to a compiled regex. Order matters: more specific patterns are matched first.
    /// </summary>
    private static readonly List<PhiPattern> PhiPatterns = new()
    {
        // SSN: XXX-XX-XXXX (with or without dashes)
        new PhiPattern(
            "SSN",
            new Regex(
                @"\b\d{3}-\d{2}-\d{4}\b",
                RegexOptions.Compiled)),

        // SSN without dashes (9 consecutive digits that look like SSN context)
        new PhiPattern(
            "SSN",
            new Regex(
                @"(?i)(?:ssn|social\s*security)[\s:]*(\d{9})\b",
                RegexOptions.Compiled)),

        // Medical Record Number (MRN): common patterns like MRN-123456, MRN: 123456, MRN#123456
        new PhiPattern(
            "MRN",
            new Regex(
                @"(?i)(?:mrn|medical\s*record\s*(?:number|num|no|#))[\s:#-]*([A-Z0-9]{4,12})\b",
                RegexOptions.Compiled)),

        // Date of birth patterns: DOB: MM/DD/YYYY, DOB: YYYY-MM-DD, Date of Birth: etc.
        new PhiPattern(
            "DOB",
            new Regex(
                @"(?i)(?:d\.?o\.?b\.?|date\s*of\s*birth)[\s:]*(\d{1,2}[/\-]\d{1,2}[/\-]\d{2,4})",
                RegexOptions.Compiled)),

        // Date of birth patterns with named months
        new PhiPattern(
            "DOB",
            new Regex(
                @"(?i)(?:d\.?o\.?b\.?|date\s*of\s*birth)[\s:]*(\w+\s+\d{1,2},?\s+\d{4})",
                RegexOptions.Compiled)),

        // Patient/person names following label patterns
        new PhiPattern(
            "NAME",
            new Regex(
                @"(?i)(?:patient|patient\s*name|name)[\s:]+([A-Z][a-z]+(?:\s+[A-Z][a-z]+){1,3})",
                RegexOptions.Compiled)),

        // Email addresses
        new PhiPattern(
            "EMAIL",
            new Regex(
                @"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b",
                RegexOptions.Compiled)),

        // US phone numbers: (XXX) XXX-XXXX, XXX-XXX-XXXX, XXX.XXX.XXXX, +1XXXXXXXXXX, etc.
        new PhiPattern(
            "PHONE",
            new Regex(
                @"(?:\+?1[\s.-]?)?\(?\d{3}\)?[\s.\-]?\d{3}[\s.\-]?\d{4}\b",
                RegexOptions.Compiled)),
    };

    public PhiRedactor(ILogger<PhiRedactor> logger)
    {
        _logger = logger;
    }

    public Task<string> RedactTextAsync(string text)
    {
        return Task.Run(() =>
        {
            if (string.IsNullOrWhiteSpace(text))
                return text;

            var redactedText = text;
            var totalRedactions = 0;

            foreach (var pattern in PhiPatterns)
            {
                var matches = pattern.Regex.Matches(redactedText);
                if (matches.Count > 0)
                {
                    totalRedactions += matches.Count;

                    // Replace full match for simple patterns, or the captured group for label-based patterns
                    if (pattern.Regex.GetGroupNumbers().Length > 1)
                    {
                        // Has capturing groups - replace the captured portion while preserving label
                        redactedText = pattern.Regex.Replace(redactedText, match =>
                        {
                            if (match.Groups.Count > 1 && match.Groups[1].Success)
                            {
                                var prefix = match.Value[..match.Groups[1].Index - match.Index];
                                return $"{prefix}[REDACTED-{pattern.Category}]";
                            }
                            return $"[REDACTED-{pattern.Category}]";
                        });
                    }
                    else
                    {
                        // No capturing groups - replace full match
                        redactedText = pattern.Regex.Replace(redactedText, $"[REDACTED-{pattern.Category}]");
                    }
                }
            }

            if (totalRedactions > 0)
            {
                _logger.LogInformation("Redacted {Count} PHI occurrences from text", totalRedactions);
            }

            return redactedText;
        });
    }

    public Task<bool> ContainsPhiAsync(string text)
    {
        return Task.Run(() =>
        {
            if (string.IsNullOrWhiteSpace(text))
                return false;

            foreach (var pattern in PhiPatterns)
            {
                if (pattern.Regex.IsMatch(text))
                {
                    _logger.LogDebug("PHI detected: category {Category}", pattern.Category);
                    return true;
                }
            }

            return false;
        });
    }

    /// <summary>
    /// Associates a PHI category label with a compiled regex pattern.
    /// </summary>
    private sealed record PhiPattern(string Category, Regex Regex);
}
