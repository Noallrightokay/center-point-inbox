using System.Text;
using DocumentFormat.OpenXml;
using DocumentFormat.OpenXml.Packaging;
using DocumentFormat.OpenXml.Spreadsheet;
using DocumentFormat.OpenXml.Wordprocessing;
using Microsoft.Extensions.Logging;

namespace CenterPointInbox.TranslationWorker.Services;

/// <summary>
/// Converts documents between formats using DocumentFormat.OpenXml.
/// Supported conversions:
///   - Word (.docx) to HTML
///   - HTML to Word (.docx)
///   - Excel (.xlsx) to CSV
/// </summary>
public class DocumentConverter : IDocumentConverter
{
    private const string MimeDocx = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    private const string MimeHtml = "text/html";
    private const string MimeXlsx = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
    private const string MimeCsv = "text/csv";

    private static readonly HashSet<(string Source, string Target)> SupportedConversions = new()
    {
        (MimeDocx, MimeHtml),
        (MimeHtml, MimeDocx),
        (MimeXlsx, MimeCsv)
    };

    private readonly ILogger<DocumentConverter> _logger;

    public DocumentConverter(ILogger<DocumentConverter> logger)
    {
        _logger = logger;
    }

    public bool IsConversionSupported(string sourceFormat, string targetFormat)
    {
        return SupportedConversions.Contains((sourceFormat, targetFormat));
    }

    public async Task<byte[]> ConvertAsync(byte[] source, string sourceFormat, string targetFormat)
    {
        if (!IsConversionSupported(sourceFormat, targetFormat))
        {
            throw new NotSupportedException(
                $"Conversion from '{sourceFormat}' to '{targetFormat}' is not supported. " +
                $"Supported conversions: {string.Join(", ", SupportedConversions.Select(c => $"{c.Source} -> {c.Target}"))}");
        }

        _logger.LogInformation(
            "Starting document conversion from {SourceFormat} to {TargetFormat}, source size: {SourceSize} bytes",
            sourceFormat, targetFormat, source.Length);

        var result = (sourceFormat, targetFormat) switch
        {
            (MimeDocx, MimeHtml) => await ConvertWordToHtmlAsync(source),
            (MimeHtml, MimeDocx) => await ConvertHtmlToWordAsync(source),
            (MimeXlsx, MimeCsv) => await ConvertExcelToCsvAsync(source),
            _ => throw new NotSupportedException($"Conversion from '{sourceFormat}' to '{targetFormat}' is not supported.")
        };

        _logger.LogInformation(
            "Completed document conversion from {SourceFormat} to {TargetFormat}, output size: {OutputSize} bytes",
            sourceFormat, targetFormat, result.Length);

        return result;
    }

    private Task<byte[]> ConvertWordToHtmlAsync(byte[] source)
    {
        return Task.Run(() =>
        {
            using var memoryStream = new MemoryStream(source);

            WordprocessingDocument wordDoc;
            try
            {
                wordDoc = WordprocessingDocument.Open(memoryStream, false);
            }
            catch (Exception ex)
            {
                throw new InvalidOperationException(
                    "Failed to open the source document as a valid Word (.docx) file.", ex);
            }

            using (wordDoc)
            {
                var body = wordDoc.MainDocumentPart?.Document?.Body;
                if (body is null)
                {
                    throw new InvalidOperationException(
                        "The Word document does not contain a valid document body.");
                }

                var htmlBuilder = new StringBuilder();
                htmlBuilder.AppendLine("<!DOCTYPE html>");
                htmlBuilder.AppendLine("<html>");
                htmlBuilder.AppendLine("<head><meta charset=\"utf-8\"></head>");
                htmlBuilder.AppendLine("<body>");

                foreach (var element in body.Elements())
                {
                    ProcessWordElement(element, htmlBuilder);
                }

                htmlBuilder.AppendLine("</body>");
                htmlBuilder.AppendLine("</html>");

                return Encoding.UTF8.GetBytes(htmlBuilder.ToString());
            }
        });
    }

    private static void ProcessWordElement(OpenXmlElement element, StringBuilder htmlBuilder)
    {
        switch (element)
        {
            case Paragraph paragraph:
                ProcessParagraph(paragraph, htmlBuilder);
                break;
            case Table table:
                ProcessTable(table, htmlBuilder);
                break;
            default:
                // Skip unsupported elements silently
                break;
        }
    }

    private static void ProcessParagraph(Paragraph paragraph, StringBuilder htmlBuilder)
    {
        var styleId = paragraph.ParagraphProperties?.ParagraphStyleId?.Val?.Value;
        var tag = GetHtmlTagForStyle(styleId);

        htmlBuilder.Append($"<{tag}>");

        foreach (var run in paragraph.Elements<Run>())
        {
            var text = run.InnerText;
            if (string.IsNullOrEmpty(text))
                continue;

            var isBold = run.RunProperties?.Bold is not null;
            var isItalic = run.RunProperties?.Italic is not null;
            var isUnderline = run.RunProperties?.Underline is not null
                && run.RunProperties.Underline.Val is not null
                && run.RunProperties.Underline.Val != UnderlineValues.None;

            if (isBold) htmlBuilder.Append("<strong>");
            if (isItalic) htmlBuilder.Append("<em>");
            if (isUnderline) htmlBuilder.Append("<u>");

            htmlBuilder.Append(System.Net.WebUtility.HtmlEncode(text));

            if (isUnderline) htmlBuilder.Append("</u>");
            if (isItalic) htmlBuilder.Append("</em>");
            if (isBold) htmlBuilder.Append("</strong>");
        }

        htmlBuilder.AppendLine($"</{tag}>");
    }

    private static string GetHtmlTagForStyle(string? styleId)
    {
        return styleId?.ToLowerInvariant() switch
        {
            "heading1" => "h1",
            "heading2" => "h2",
            "heading3" => "h3",
            "heading4" => "h4",
            "heading5" => "h5",
            "heading6" => "h6",
            "title" => "h1",
            "subtitle" => "h2",
            _ => "p"
        };
    }

    private static void ProcessTable(Table table, StringBuilder htmlBuilder)
    {
        htmlBuilder.AppendLine("<table>");

        foreach (var row in table.Elements<TableRow>())
        {
            htmlBuilder.Append("<tr>");

            foreach (var cell in row.Elements<TableCell>())
            {
                htmlBuilder.Append("<td>");

                var cellText = string.Join(" ", cell.Elements<Paragraph>()
                    .Select(p => p.InnerText));

                htmlBuilder.Append(System.Net.WebUtility.HtmlEncode(cellText));
                htmlBuilder.Append("</td>");
            }

            htmlBuilder.AppendLine("</tr>");
        }

        htmlBuilder.AppendLine("</table>");
    }

    private Task<byte[]> ConvertHtmlToWordAsync(byte[] source)
    {
        return Task.Run(() =>
        {
            var html = Encoding.UTF8.GetString(source);

            using var memoryStream = new MemoryStream();
            using (var wordDoc = WordprocessingDocument.Create(memoryStream, WordprocessingDocumentType.Document, true))
            {
                var mainPart = wordDoc.AddMainDocumentPart();
                mainPart.Document = new Document();
                var body = mainPart.Document.AppendChild(new Body());

                // Parse HTML content into Word paragraphs
                var lines = ExtractTextLinesFromHtml(html);

                foreach (var line in lines)
                {
                    var paragraph = new Paragraph();
                    var run = new Run();

                    if (line.IsBold)
                    {
                        run.RunProperties = new RunProperties();
                        run.RunProperties.AppendChild(new Bold());
                    }

                    if (line.IsHeading)
                    {
                        paragraph.ParagraphProperties = new ParagraphProperties();
                        paragraph.ParagraphProperties.ParagraphStyleId = new ParagraphStyleId
                        {
                            Val = $"Heading{line.HeadingLevel}"
                        };
                    }

                    run.AppendChild(new Text(line.Text) { Space = SpaceProcessingModeValues.Preserve });
                    paragraph.AppendChild(run);
                    body.AppendChild(paragraph);
                }

                // Add at least one empty paragraph if no content was extracted
                if (!lines.Any())
                {
                    body.AppendChild(new Paragraph(new Run(new Text(""))));
                }

                mainPart.Document.Save();
            }

            return memoryStream.ToArray();
        });
    }

    private static List<HtmlLine> ExtractTextLinesFromHtml(string html)
    {
        var lines = new List<HtmlLine>();

        // Strip full HTML structure tags to get inner content
        var content = html;
        content = System.Text.RegularExpressions.Regex.Replace(content, @"<!DOCTYPE[^>]*>", "", System.Text.RegularExpressions.RegexOptions.IgnoreCase);
        content = System.Text.RegularExpressions.Regex.Replace(content, @"<html[^>]*>|</html>", "", System.Text.RegularExpressions.RegexOptions.IgnoreCase);
        content = System.Text.RegularExpressions.Regex.Replace(content, @"<head[^>]*>.*?</head>", "", System.Text.RegularExpressions.RegexOptions.IgnoreCase | System.Text.RegularExpressions.RegexOptions.Singleline);
        content = System.Text.RegularExpressions.Regex.Replace(content, @"<body[^>]*>|</body>", "", System.Text.RegularExpressions.RegexOptions.IgnoreCase);

        // Extract headings
        var headingPattern = new System.Text.RegularExpressions.Regex(
            @"<h([1-6])[^>]*>(.*?)</h\1>",
            System.Text.RegularExpressions.RegexOptions.IgnoreCase | System.Text.RegularExpressions.RegexOptions.Singleline);

        // Extract paragraphs
        var paragraphPattern = new System.Text.RegularExpressions.Regex(
            @"<p[^>]*>(.*?)</p>",
            System.Text.RegularExpressions.RegexOptions.IgnoreCase | System.Text.RegularExpressions.RegexOptions.Singleline);

        // Process content sequentially using a combined pattern
        var combinedPattern = new System.Text.RegularExpressions.Regex(
            @"<(h[1-6]|p|div|li|td)[^>]*>(.*?)</\1>",
            System.Text.RegularExpressions.RegexOptions.IgnoreCase | System.Text.RegularExpressions.RegexOptions.Singleline);

        foreach (System.Text.RegularExpressions.Match match in combinedPattern.Matches(content))
        {
            var tagName = match.Groups[1].Value.ToLowerInvariant();
            var innerHtml = match.Groups[2].Value;

            // Strip inner HTML tags to get plain text
            var plainText = System.Text.RegularExpressions.Regex.Replace(innerHtml, @"<[^>]+>", "");
            plainText = System.Net.WebUtility.HtmlDecode(plainText).Trim();

            if (string.IsNullOrWhiteSpace(plainText))
                continue;

            var isBold = innerHtml.Contains("<strong>", StringComparison.OrdinalIgnoreCase)
                || innerHtml.Contains("<b>", StringComparison.OrdinalIgnoreCase);

            var isHeading = tagName.StartsWith('h') && tagName.Length == 2 && char.IsDigit(tagName[1]);
            var headingLevel = isHeading ? int.Parse(tagName[1].ToString()) : 0;

            lines.Add(new HtmlLine
            {
                Text = plainText,
                IsBold = isBold,
                IsHeading = isHeading,
                HeadingLevel = headingLevel
            });
        }

        // If no structured elements found, treat entire content as plain text
        if (!lines.Any())
        {
            var plainText = System.Text.RegularExpressions.Regex.Replace(content, @"<[^>]+>", "");
            plainText = System.Net.WebUtility.HtmlDecode(plainText).Trim();

            if (!string.IsNullOrWhiteSpace(plainText))
            {
                foreach (var segment in plainText.Split('\n', StringSplitOptions.RemoveEmptyEntries))
                {
                    var trimmed = segment.Trim();
                    if (!string.IsNullOrWhiteSpace(trimmed))
                    {
                        lines.Add(new HtmlLine { Text = trimmed });
                    }
                }
            }
        }

        return lines;
    }

    private Task<byte[]> ConvertExcelToCsvAsync(byte[] source)
    {
        return Task.Run(() =>
        {
            using var memoryStream = new MemoryStream(source);

            SpreadsheetDocument spreadsheetDoc;
            try
            {
                spreadsheetDoc = SpreadsheetDocument.Open(memoryStream, false);
            }
            catch (Exception ex)
            {
                throw new InvalidOperationException(
                    "Failed to open the source document as a valid Excel (.xlsx) file.", ex);
            }

            using (spreadsheetDoc)
            {
                var workbookPart = spreadsheetDoc.WorkbookPart
                    ?? throw new InvalidOperationException("The Excel file does not contain a workbook.");

                var sheet = workbookPart.Workbook.Sheets?.Elements<Sheet>().FirstOrDefault()
                    ?? throw new InvalidOperationException("The Excel file does not contain any worksheets.");

                var worksheetPart = (WorksheetPart)workbookPart.GetPartById(sheet.Id!.Value!);
                var sheetData = worksheetPart.Worksheet.Elements<SheetData>().FirstOrDefault()
                    ?? throw new InvalidOperationException("The worksheet does not contain any data.");

                var sharedStrings = workbookPart.SharedStringTablePart?.SharedStringTable;

                var csvBuilder = new StringBuilder();

                foreach (var row in sheetData.Elements<Row>())
                {
                    var cellValues = new List<string>();

                    foreach (var cell in row.Elements<Cell>())
                    {
                        var value = GetCellValue(cell, sharedStrings);
                        cellValues.Add(EscapeCsvField(value));
                    }

                    csvBuilder.AppendLine(string.Join(",", cellValues));
                }

                return Encoding.UTF8.GetBytes(csvBuilder.ToString());
            }
        });
    }

    private static string GetCellValue(Cell cell, SharedStringTable? sharedStrings)
    {
        if (cell.CellValue is null)
            return string.Empty;

        var value = cell.CellValue.InnerText;

        if (cell.DataType?.Value == CellValues.SharedString && sharedStrings is not null)
        {
            if (int.TryParse(value, out var index))
            {
                var sharedStringItem = sharedStrings.Elements<SharedStringItem>().ElementAtOrDefault(index);
                return sharedStringItem?.InnerText ?? value;
            }
        }

        return value;
    }

    private static string EscapeCsvField(string field)
    {
        if (string.IsNullOrEmpty(field))
            return string.Empty;

        if (field.Contains(',') || field.Contains('"') || field.Contains('\n') || field.Contains('\r'))
        {
            return $"\"{field.Replace("\"", "\"\"")}\"";
        }

        return field;
    }

    /// <summary>
    /// Represents a line of text extracted from HTML with formatting metadata.
    /// </summary>
    private sealed class HtmlLine
    {
        public string Text { get; init; } = string.Empty;
        public bool IsBold { get; init; }
        public bool IsHeading { get; init; }
        public int HeadingLevel { get; init; }
    }
}
