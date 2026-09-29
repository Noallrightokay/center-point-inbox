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
/// one space, and none is left before the last extension. And nothing that
/// draws nothing is kept ([`invisible`]): `invoice.pdf<U+200B>.exe` showed as
/// `invoice.pdf`, and was not flagged, because the zero-width space sat
/// between the two extensions.
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
            && !invisible(*c)
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
/// alike (`invoice.pdf<spaces>.exe`, `invoice.pdf.exe.` included). A
/// character drawn as a full stop counts as one here ([`looks_like_a_dot`]):
/// `invoice<U+2024>pdf.exe` is a program Windows shows as `invoice.pdf`.
pub fn looks_disguised(name: &str) -> bool {
    // Documents a customer expects to open, and program types Windows runs,
    // merges into the registry, follows or mounts on a double-click (review
    // P3-4 added csv, rtf, html and htm; cpl, reg, url, iso and one).
    const DOCUMENT: [&str; 17] = [
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "jpg", "jpeg", "png", "gif", "txt",
        "zip", "csv", "rtf", "html", "htm",
    ];
    const PROGRAM: [&str; 27] = [
        "exe", "msi", "bat", "cmd", "scr", "ps1", "js", "jse", "vbs", "vbe", "wsf", "wsh", "hta",
        "jar", "com", "pif", "lnk", "app", "dmg", "pkg", "sh", "command", "cpl", "reg", "url",
        "iso", "one",
    ];
    let clean = safe_file_name(name);
    let mut parts = clean.rsplit(|c: char| c == '.' || looks_like_a_dot(c));
    // Three parts at least: a name, then two extensions. The cleaned name
    // never starts with a dot, so the name part is never empty.
    let (Some(last), Some(before), Some(_)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    DOCUMENT.iter().any(|e| before.eq_ignore_ascii_case(e))
        && PROGRAM.iter().any(|e| last.eq_ignore_ascii_case(e))
}

/// Characters that draw nothing: every format character (Unicode category
/// Cf), and the rest of Default_Ignorable_Code_Point (the combining grapheme
/// joiner, variation selectors, Khmer and Mongolian ones). Rust's
/// `is_control` covers only category Cc. The Hangul fillers are left to
/// `blank`, which turns them into a space. The bidirectional controls are Cf
/// too and are also named where they are removed. Taking U+200C and U+200D
/// out of a Persian name or an emoji sequence is a small cost in a file name.
fn invisible(c: char) -> bool {
    matches!(
        c as u32,
        // Category Cf (Unicode 16).
        0x00ad
            | 0x0600..=0x0605
            | 0x061c
            | 0x06dd
            | 0x070f
            | 0x0890..=0x0891
            | 0x08e2
            | 0x180e
            | 0x200b..=0x200f
            | 0x202a..=0x202e
            | 0x2060..=0x2064
            | 0x2066..=0x206f
            | 0xfeff
            | 0xfff9..=0xfffb
            | 0x110bd
            | 0x110cd
            | 0x13430..=0x1343f
            | 0x1bca0..=0x1bca3
            | 0x1d173..=0x1d17a
            | 0xe0001
            | 0xe0020..=0xe007f
            // The rest of Default_Ignorable_Code_Point, less the Hangul
            // fillers: reserved, unassigned or drawing nothing.
            | 0x034f
            | 0x17b4..=0x17b5
            | 0x180b..=0x180f
            | 0x2065
            | 0xfe00..=0xfe0f
            | 0xfff0..=0xfff8
            | 0xe0000..=0xe0fff
    )
}

/// Characters drawn as a full stop that Windows does not take for one, so
/// the extension it hides is whatever follows the last real dot.
fn looks_like_a_dot(c: char) -> bool {
    matches!(
        c,
        '\u{2024}' // one dot leader
            | '\u{fe52}' // small full stop
            | '\u{ff0e}' // fullwidth full stop
            | '\u{0701}' // Syriac supralinear full stop
            | '\u{0702}' // Syriac sublinear full stop
            | '\u{a4f8}' // Lisu letter tone mya ti
            | '\u{10a50}' // Kharoshthi punctuation dot
            | '\u{2e31}' // word separator middle dot
            | '\u{00b7}' // middle dot
            | '\u{2027}' // hyphenation point
            | '\u{0387}' // Greek ano teleia
    )
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
    fn nothing_invisible_or_dot_shaped_hides_a_program() {
        // Review P3-1: a character that draws nothing, or one drawn as a full
        // stop that Windows does not read as one, kept `invoice.pdf.exe` from
        // being flagged while Explorer still showed it as `invoice.pdf`.
        for name in [
            "invoice.pdf\u{200b}.exe",
            "invoice.pd\u{200b}f.exe",
            "invoice.pdf\u{2060}.exe",
            "invoice.pdf\u{feff}.exe",
            "invoice.pdf\u{ad}.exe",
            "invoice.pdf\u{180e}.exe",
            "invoice.pdf\u{e0020}.exe",
            "invoice\u{2024}pdf.exe",
            "invoice\u{ff0e}pdf.exe",
            "invoice\u{fe52}pdf.exe",
        ] {
            assert!(looks_disguised(name), "{name:?}");
            assert!(looks_disguised(&safe_file_name(name)), "{name:?}");
        }
        assert_eq!(safe_file_name("invoice.pdf\u{200b}.exe"), "invoice.pdf.exe");
        // Every format character (Unicode category Cf) goes from the name, and
        // so does every other character that draws nothing.
        for c in [
            '\u{ad}',
            '\u{600}',
            '\u{605}',
            '\u{6dd}',
            '\u{70f}',
            '\u{890}',
            '\u{8e2}',
            '\u{180e}',
            '\u{200b}',
            '\u{200c}',
            '\u{200d}',
            '\u{2060}',
            '\u{2064}',
            '\u{206a}',
            '\u{206f}',
            '\u{feff}',
            '\u{fff9}',
            '\u{fffb}',
            '\u{110bd}',
            '\u{110cd}',
            '\u{13430}',
            '\u{1343f}',
            '\u{1bca0}',
            '\u{1d173}',
            '\u{e0001}',
            '\u{e007f}',
            '\u{34f}',
            '\u{17b4}',
            '\u{180b}',
            '\u{fe0f}',
            '\u{e0100}',
        ] {
            assert_eq!(
                safe_file_name(&format!("re{c}port.pdf")),
                "report.pdf",
                "U+{:04X}",
                c as u32
            );
        }
        // A look-alike dot stays in the name (it is what was sent, and Windows
        // takes the part after the real dot as the extension)...
        assert_eq!(
            safe_file_name("invoice\u{2024}pdf.exe"),
            "invoice\u{2024}pdf.exe"
        );
        // ...and in a name that is not dressed up it changes nothing.
        for name in [
            "Q3\u{b7}figures.pdf",
            "notes\u{2024}v2.txt",
            "setup\u{ff0e}x64.exe",
            "invoice\u{2024}exe.pdf",
        ] {
            assert!(!looks_disguised(name), "{name:?}");
        }
    }

    #[test]
    fn the_wider_lists_catch_more_disguises_and_leave_honest_names_alone() {
        // Review P3-4: program types Windows runs or mounts on a double-click,
        // and documents a customer expects to open.
        for name in [
            "invoice.pdf.cpl",
            "settings.txt.reg",
            "link.pdf.url",
            "invoice.pdf.iso",
            "notes.docx.one",
            "report.csv.exe",
            "letter.rtf.exe",
            "page.html.exe",
            "page.htm.scr",
            "REPORT.CSV.ISO",
        ] {
            assert!(looks_disguised(name), "{name}");
        }
        for name in [
            "setup.exe",
            "backup.iso",
            "tweak.reg",
            "shortcut.url",
            "notebook.one",
            "control.cpl",
            "report.csv",
            "page.html",
            "v2.1.iso",
            "invoice.pdf.csv",
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
