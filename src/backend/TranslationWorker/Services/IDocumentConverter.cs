namespace CenterPointInbox.TranslationWorker.Services;

/// <summary>
/// Service for converting documents between formats.
/// Supports Word (OOXML) to HTML, HTML to Word, and Excel to CSV conversions.
/// </summary>
public interface IDocumentConverter
{
    /// <summary>
    /// Converts document content from one format to another.
    /// </summary>
    /// <param name="source">The raw bytes of the source document.</param>
    /// <param name="sourceFormat">The MIME type of the source document.</param>
    /// <param name="targetFormat">The desired MIME type for the output document.</param>
    /// <returns>The converted document as a byte array.</returns>
    /// <exception cref="NotSupportedException">Thrown when the conversion pair is not supported.</exception>
    /// <exception cref="InvalidOperationException">Thrown when the source document is malformed.</exception>
    Task<byte[]> ConvertAsync(byte[] source, string sourceFormat, string targetFormat);

    /// <summary>
    /// Returns whether the specified conversion is supported.
    /// </summary>
    /// <param name="sourceFormat">The MIME type of the source document.</param>
    /// <param name="targetFormat">The desired MIME type for the output document.</param>
    /// <returns>True if the conversion is supported; otherwise false.</returns>
    bool IsConversionSupported(string sourceFormat, string targetFormat);
}
