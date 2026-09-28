//! A sender's file name, judged and made safe.
//!
//! An attachment's name is the sender's, so it is treated as hostile twice:
//! once when a file is created from it ([`safe_file_name`]), and once when the
//! reading pane lists it ([`looks_disguised`]), since a name can dress a
//! program up as a document and the customer should hear so before saving it.

/// An attachment's name made safe to create in the Downloads folder.
///
/// The name is the sender's, so it is treated as hostile: no directory parts
/// (`../../.bashrc`), nothing a filesystem rejects or reads specially, no
/// Windows device names (`CON`, `NUL.txt`), and no bidirectional-text controls
/// — `invoice\u{202e}fdp.exe` displays as `invoiceexe.pdf`, which is how a
/// program passes itself off as a PDF. Nor can blank space hide the real
/// extension: a file dialog cuts a long name at its end, so
/// `invoice.pdf<35 spaces>.exe` read as `invoice.pdf`. Every run of blanks is
/// one space, and none is left before the last extension.
pub fn safe_file_name(name: &str) -> String {
    // Every Unicode space, and the characters fonts draw as a gap: the Hangul
    // fillers and the blank Braille cell.
    fn blank(c: char) -> bool {
        c.is_whitespace()
            || matches!(
                c,
                '\u{115f}' | '\u{1160}' | '\u{3164}' | '\u{ffa0}' | '\u{2800}'
            )
    }
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut clean = String::with_capacity(base.len());
    for c in base.chars().filter(|c| {
        !c.is_control()
            && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')
            && !matches!(*c as u32, 0x200e | 0x200f | 0x202a..=0x202e | 0x2066..=0x2069 | 0x061c)
    }) {
        if !blank(c) {
            clean.push(c);
        } else if !clean.ends_with(' ') {
            clean.push(' ');
        }
    }
    clean = clean
        .trim_matches(|c: char| c == '.' || c == ' ')
        .to_string();
    if let Some(i) = clean.rfind('.') {
        clean = format!("{}{}", clean[..i].trim_end(), &clean[i..]);
    }
    let stem = clean
        .split('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    // COM and LPT with one digit, or with a superscript ¹ ² ³, which Windows
    // reserves too.
    let numbered = (stem.starts_with("COM") || stem.starts_with("LPT"))
        && (matches!(&stem.as_bytes()[3..], [d] if d.is_ascii_digit())
            || matches!(&stem[3..], "\u{b9}" | "\u{b2}" | "\u{b3}"));
    let reserved = numbered
        || matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        );
    if reserved {
        clean.insert(0, '_');
    }
    if clean.is_empty() {
        return "attachment".into();
    }
    // Long enough for any real name, short enough for every filesystem.
    if clean.len() > 150 {
        let ext = clean
            .rfind('.')
            .map(|i| clean[i..].to_string())
            .filter(|e| e.len() <= 12)
            .unwrap_or_default();
        let mut keep = 150 - ext.len();
        while !clean.is_char_boundary(keep) {
            keep -= 1;
        }
        clean = format!("{}{ext}", clean[..keep].trim_end());
    }
    clean
}

/// Whether a file's name dresses a program up as a document:
/// `invoice.pdf.exe`, which Windows shows as `invoice.pdf` while it hides
/// known extensions, as it does by default. `safe_file_name` keeps such a name
/// as it is, since the customer must be able to save what was sent; this is
/// how the reading pane can tell, and ask before saving. It judges the name
/// `safe_file_name` gives, so the sender's name and the cleaned one answer
/// alike (`invoice.pdf<spaces>.exe`, `invoice.pdf.exe.` included).
pub fn looks_disguised(name: &str) -> bool {
    const DOCUMENT: [&str; 13] = [
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "jpg", "jpeg", "png", "gif", "txt",
        "zip",
    ];
    const PROGRAM: [&str; 22] = [
        "exe", "msi", "bat", "cmd", "scr", "ps1", "js", "jse", "vbs", "vbe", "wsf", "wsh", "hta",
        "jar", "com", "pif", "lnk", "app", "dmg", "pkg", "sh", "command",
    ];
    let clean = safe_file_name(name);
    let mut parts = clean.rsplit('.');
    // Three parts at least: a name, then two extensions. The cleaned name
    // never starts with a dot, so the name part is never empty.
    let (Some(last), Some(before), Some(_)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    DOCUMENT.iter().any(|e| before.eq_ignore_ascii_case(e))
        && PROGRAM.iter().any(|e| last.eq_ignore_ascii_case(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attachment_name_cannot_escape_or_disguise_itself() {
        assert_eq!(safe_file_name("../../.bashrc"), "bashrc");
        assert_eq!(safe_file_name("C:\\Windows\\evil.dll"), "evil.dll");
        assert_eq!(safe_file_name("invoice\u{202e}fdp.exe"), "invoicefdp.exe");
        assert_eq!(safe_file_name("CON"), "_CON");
        assert_eq!(safe_file_name("nul.txt"), "_nul.txt");
        assert_eq!(safe_file_name("COM3.pdf"), "_COM3.pdf");
        assert_eq!(safe_file_name("what?:<>|*\".pdf"), "what.pdf");
        assert_eq!(safe_file_name(" . "), "attachment");
        assert_eq!(safe_file_name("Q3 figures.pdf"), "Q3 figures.pdf");
        assert_eq!(safe_file_name("résumé.docx"), "résumé.docx");
        let long = safe_file_name(&format!("{}.pdf", "é".repeat(200)));
        assert!(long.len() <= 150 && long.ends_with(".pdf"), "{long}");
    }

    #[test]
    fn every_windows_device_name_is_defused_with_or_without_an_extension() {
        // Review finding 12: superscript digits and the console devices.
        for name in [
            "COM\u{b9}",
            "com\u{b2}.txt",
            "Com\u{b3}.pdf",
            "LPT\u{b9}.doc",
            "lpt\u{b2}",
            "LPT\u{b3}.tar.gz",
            "CONIN$",
            "conin$.txt",
            "CONOUT$",
            "ConOut$.log",
        ] {
            assert_eq!(safe_file_name(name), format!("_{name}"), "{name}");
        }
        // The ones already caught, extension or not, in any case.
        for name in [
            "CON",
            "CON.txt",
            "con.txt",
            "PRN.pdf",
            "aux",
            "NUL.tar.gz",
            "COM1",
            "lpt9.txt",
            "COM0.x",
        ] {
            assert_eq!(safe_file_name(name), format!("_{name}"), "{name}");
        }
        // Blanks before the extension do not get a device name past the check.
        assert_eq!(safe_file_name("CON .txt"), "_CON.txt");
        assert_eq!(safe_file_name("conin$ .txt"), "_conin$.txt");
        // Near misses are ordinary names.
        for name in [
            "COM10.txt",
            "COM\u{2074}.txt",
            "LPT.txt",
            "CONSOLE.txt",
            "CONIN.txt",
            "icon.png",
        ] {
            assert_eq!(safe_file_name(name), name, "{name}");
        }
    }

    #[test]
    fn a_program_dressed_as_a_document_is_named_as_it_is_and_flagged() {
        // The real extension is kept, so what was sent can still be saved...
        assert_eq!(safe_file_name("invoice.pdf.exe"), "invoice.pdf.exe");
        // ...and the disguise is reported, in any case, however padded.
        for name in [
            "invoice.pdf.exe",
            "INVOICE.PDF.EXE",
            "scan.JPG.scr",
            "report.docx.js",
            "q3.xlsx.bat",
            "notes.txt.cmd",
            "photo.jpeg.lnk",
            "pics.zip.msi",
            "deck.pptx.jar",
            "a.png.hta",
            "a.gif.vbs",
            "a.doc.ps1",
            "a.xls.wsf",
            "a.ppt.pif",
            "a.pdf.com",
            "a.pdf.jse",
            "a.pdf.vbe",
            "a.pdf.wsh",
            "a.pdf.app",
            "a.pdf.dmg",
            "a.pdf.pkg",
            "a.pdf.sh",
            "a.pdf.command",
            "../../invoice.pdf.exe",
            "invoice.pdf .exe",
            "invoice.pdf\u{202e}.exe",
            "invoice.pdf.exe.",
        ] {
            assert!(looks_disguised(name), "{name}");
            assert!(looks_disguised(&safe_file_name(name)), "{name}");
        }
        let padded = format!("invoice.pdf{}.exe", "\u{3000}".repeat(35));
        assert!(looks_disguised(&padded));
        // A program that says what it is, or a document with dots in it, is not.
        for name in [
            "setup.exe",
            "invoice.pdf",
            "archive.tar.gz",
            "v1.2.exe",
            "pdf.exe",
            ".pdf.exe",
            "invoice.pdf.txt",
            "invoice.exe.pdf",
            "invoice.pdf. exe",
            "invoice.pdfx.exe",
            "",
        ] {
            assert!(!looks_disguised(name), "{name}");
        }
    }

    #[test]
    fn no_run_of_blanks_can_push_the_real_extension_out_of_view() {
        // A file dialog cuts a long name at its end; padding hides `.exe`.
        let padded = format!("invoice.pdf{}.exe", " ".repeat(35));
        assert_eq!(safe_file_name(&padded), "invoice.pdf.exe");
        assert_eq!(safe_file_name("invoice.pdf .exe"), "invoice.pdf.exe");
        assert_eq!(safe_file_name("Q3    figures.pdf"), "Q3 figures.pdf");
        // Blanks that are not ASCII spaces pad just as well.
        assert_eq!(
            safe_file_name("invoice.pdf\u{a0}\u{2003}\u{3000}\u{3164}\u{2800}.exe"),
            "invoice.pdf.exe"
        );
        assert_eq!(safe_file_name("a \u{a0} b.txt"), "a b.txt");
        // Only the blanks before the last extension go; the name keeps its dots.
        assert_eq!(safe_file_name("notes .v2 .txt"), "notes .v2.txt");
        // Cutting to 150 bytes cannot leave a blank before the extension.
        let cut = safe_file_name(&format!("{} b.pdf", "a".repeat(145)));
        assert!(cut.len() <= 150 && cut.ends_with(".pdf"), "{cut}");
        assert!(!cut.contains(" .pdf"), "{cut}");
    }
}
