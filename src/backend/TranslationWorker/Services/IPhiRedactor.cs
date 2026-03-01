namespace CenterPointInbox.TranslationWorker.Services;

/// <summary>
/// Service for detecting and redacting Protected Health Information (PHI) from text content.
/// Implements pattern-based detection for common PHI types including SSNs, phone numbers,
/// email addresses, medical record numbers, dates of birth, and patient names.
/// </summary>
public interface IPhiRedactor
{
    /// <summary>
    /// Scans the input text and replaces all detected PHI with typed redaction placeholders.
    /// Each match is replaced with [REDACTED-{type}] where type indicates the PHI category.
    /// </summary>
    /// <param name="text">The text to scan and redact.</param>
    /// <returns>The text with all detected PHI replaced by redaction placeholders.</returns>
    Task<string> RedactTextAsync(string text);

    /// <summary>
    /// Checks whether the input text contains any detectable PHI patterns.
    /// </summary>
    /// <param name="text">The text to scan for PHI.</param>
    /// <returns>True if any PHI patterns are detected; otherwise false.</returns>
    Task<bool> ContainsPhiAsync(string text);
}
