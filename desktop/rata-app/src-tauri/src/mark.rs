//! Telling the operating system that a file RATA saved came from the internet.
//!
//! Browsers and other mail apps tag every download, and Windows and macOS
//! protect the customer only for tagged files: SmartScreen checks a program,
//! Office opens a document in Protected View and blocks its macros, Gatekeeper
//! checks an app. An untagged `invoice.pdf.exe` in Downloads gets none of that
//! when it is double-clicked. So every file RATA writes into Downloads (a saved
//! attachment, a Format Bridge download) is tagged here, as a browser does.
//!
//! The tag says only "from the internet". It never names where from: a browser
//! adds the page it came from, but here that would be the sender, and RATA
//! does not write who mails the customer into their filesystem.
//!
//! Marking is best effort. A save never fails because of it (a FAT32 USB stick
//! has no alternate data streams; some volumes refuse extended attributes), and
//! nothing is logged about it.

use std::path::Path;

/// The Windows `Zone.Identifier` stream: zone 3 is "Internet". No
/// `ReferrerUrl` or `HostUrl`, which would record the sender.
#[cfg(any(windows, test))]
pub fn zone_identifier_bytes() -> &'static [u8] {
    b"[ZoneTransfer]\r\nZoneId=3\r\n"
}

/// The macOS `com.apple.quarantine` value for a file downloaded at `now`
/// (seconds since 1970): flags `0081` (downloaded), the time in hex, the agent
/// that downloaded it, and no event UUID, which only links to a history
/// database RATA does not write to.
#[cfg(any(target_os = "macos", test))]
pub fn quarantine_value(now: u64) -> String {
    format!("0081;{now:08x};RATA;")
}

/// Mark `path` as downloaded from the internet. Callers ignore the result:
/// it is an error only so tests can see what happened.
#[cfg(windows)]
pub fn from_internet(path: &Path) -> std::io::Result<()> {
    use std::io::Write;
    // `file.pdf:Zone.Identifier` is the file's alternate data stream on NTFS
    // (and ReFS); on a volume without streams this fails, which is fine.
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    std::fs::File::create(stream)?.write_all(zone_identifier_bytes())
}

/// Mark `path` as downloaded from the internet. Callers ignore the result:
/// it is an error only so tests can see what happened.
#[cfg(target_os = "macos")]
pub fn from_internet(path: &Path) -> std::io::Result<()> {
    xattr::set(
        path,
        "com.apple.quarantine",
        quarantine_value(crate::store::now()).as_bytes(),
    )
}

/// Linux (and anything else): nothing. No desktop acts on a mark the way
/// Windows and macOS do, and the one that exists, `user.xdg.origin.url`,
/// would record where the file came from, which is the sender.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn from_internet(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Reading the mark back (SEC-9).
//
// RATA never marks a file it made with Create file (`created`), so a file
// at a created record's path that carries the mark is somebody else's: a
// stranger's attachment saved later under the name RATA's file left free.
// `created::checked` refuses such a file. Word, Excel and the rest save by
// writing a new file and renaming it over the old one, which carries no
// mark over from a file that had none, so the customer's own edits pass.

/// Whether `path` carries a download mark: on Windows a `Zone.Identifier`
/// stream naming the Internet or Restricted zone (or one RATA cannot read),
/// on macOS a `com.apple.quarantine` attribute whose flags say it was
/// downloaded (or one RATA cannot read). Linux has no mark, so nothing
/// there carries one. A volume that cannot hold a mark (FAT32, a share
/// refusing streams or attributes) carries none.
pub fn carries_mark(path: &Path) -> bool {
    #[cfg(test)]
    if let Some(read) = READER.with(std::cell::Cell::get) {
        return read(path);
    }
    on_this_system(path)
}

#[cfg(windows)]
fn on_this_system(path: &Path) -> bool {
    zone_marked(path)
}

#[cfg(target_os = "macos")]
fn on_this_system(path: &Path) -> bool {
    match xattr::get(path, "com.apple.quarantine") {
        Ok(Some(value)) => quarantine_says_downloaded(&value),
        _ => false,
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn on_this_system(_path: &Path) -> bool {
    false
}

/// The most of a `Zone.Identifier` stream read: a real one is a few lines.
#[cfg(any(windows, test))]
const ZONE_READ_MAX: u64 = 4096;

/// Whether `path`'s `Zone.Identifier` stream marks it, read the way
/// `from_internet` writes it, as `<path>:Zone.Identifier`. On Windows that
/// is the file's alternate data stream; anywhere else it is a file of that
/// name beside it, which is what the tests use as a stand-in.
#[cfg(any(windows, test))]
pub fn zone_marked(path: &Path) -> bool {
    use std::io::Read;
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    let Ok(file) = std::fs::File::open(stream) else {
        return false;
    };
    let mut text = Vec::new();
    if file.take(ZONE_READ_MAX).read_to_end(&mut text).is_err() {
        return true;
    }
    zone_says_internet(&text)
}

/// Whether a `Zone.Identifier` stream's text puts the file outside this
/// computer and its trusted network: `ZoneId` 3 (Internet) or 4
/// (Restricted), or no `ZoneId` RATA can read, since something wrote the
/// stream. 0 to 2 (this computer, the intranet, trusted sites) is not a
/// download from the internet.
#[cfg(any(windows, test))]
pub fn zone_says_internet(text: &[u8]) -> bool {
    let text = String::from_utf8_lossy(text);
    let zone = text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("ZoneId")
            .then(|| value.trim().parse::<u32>().ok())
    });
    !matches!(zone, Some(Some(0..=2)))
}

/// Whether a `com.apple.quarantine` value says the file was downloaded:
/// its first field, the flags in hex, has the download bit (0x0001) that
/// browsers, mail apps and RATA's own `quarantine_value` set. A value whose
/// flags cannot be read counts as downloaded. A file a sandboxed app (Word,
/// Pages) writes may carry the attribute without that bit, and it is the
/// customer's own.
#[cfg(any(target_os = "macos", test))]
pub fn quarantine_says_downloaded(value: &[u8]) -> bool {
    let value = String::from_utf8_lossy(value);
    let flags = value.split(';').next().unwrap_or("").trim();
    match u32::from_str_radix(flags, 16) {
        Ok(flags) => flags & 0x0001 != 0,
        Err(_) => true,
    }
}

/// A mark reader a test stands in for the system's.
#[cfg(test)]
pub type Reader = fn(&Path) -> bool;

#[cfg(test)]
thread_local! {
    /// A test's own mark reader for this thread, in place of the system's:
    /// on Linux there is no mark to read, so tests stand one in.
    static READER: std::cell::Cell<Option<Reader>> = const { std::cell::Cell::new(None) };
}

/// Read marks on this thread with `read` until the guard is dropped.
#[cfg(test)]
pub fn read_marks_with(read: Reader) -> ReaderGuard {
    READER.with(|r| r.set(Some(read)));
    ReaderGuard
}

#[cfg(test)]
pub struct ReaderGuard;

#[cfg(test)]
impl Drop for ReaderGuard {
    fn drop(&mut self) {
        READER.with(|r| r.set(None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_windows_stream_says_internet_and_nothing_else() {
        assert_eq!(
            zone_identifier_bytes(),
            b"[ZoneTransfer]\r\nZoneId=3\r\n".as_slice()
        );
        let text = std::str::from_utf8(zone_identifier_bytes()).unwrap();
        assert!(!text.contains("Url"), "must not record where it came from");
    }

    #[test]
    fn the_macos_attribute_is_flags_time_agent() {
        // 2026-09-28 00:00:00 UTC.
        assert_eq!(quarantine_value(1_790_553_600), "0081;6ab9ae00;RATA;");
        assert_eq!(quarantine_value(0), "0081;00000000;RATA;");
        assert_eq!(quarantine_value(0x1_0000_0000), "0081;100000000;RATA;");
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn on_linux_marking_touches_nothing() {
        let dir = std::env::temp_dir().join(format!("rata-mark-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("invoice.pdf");
        std::fs::write(&file, b"%PDF").unwrap();
        assert!(from_internet(&file).is_ok());
        assert_eq!(std::fs::read(&file).unwrap(), b"%PDF");
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("invoice.pdf")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_zone_stream_marks_the_internet_and_restricted_zones() {
        assert!(zone_says_internet(zone_identifier_bytes()));
        // What browsers write, with where it came from.
        assert!(zone_says_internet(
            b"[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://x/\r\nHostUrl=https://x/a.docx\r\n"
        ));
        assert!(zone_says_internet(b"[ZoneTransfer]\nzoneid = 4\n"));
        // Something wrote a stream RATA cannot read: marked.
        assert!(zone_says_internet(b""));
        assert!(zone_says_internet(b"[ZoneTransfer]\r\n"));
        assert!(zone_says_internet(b"[ZoneTransfer]\r\nZoneId=three\r\n"));
        // This computer, the intranet, trusted sites: not a download.
        for zone in 0..=2 {
            let text = format!("[ZoneTransfer]\r\nZoneId={zone}\r\n");
            assert!(!zone_says_internet(text.as_bytes()), "{zone}");
        }
    }

    #[test]
    fn a_quarantine_value_marks_a_download_only() {
        assert!(quarantine_says_downloaded(
            quarantine_value(1_790_553_600).as_bytes()
        ));
        // Safari and Chrome.
        assert!(quarantine_says_downloaded(b"0083;6ab9ae00;Safari;F00D"));
        assert!(quarantine_says_downloaded(b"0081;6ab9ae00;Chrome;"));
        // Unreadable flags: marked.
        assert!(quarantine_says_downloaded(b""));
        assert!(quarantine_says_downloaded(b"zz;1;x;"));
        // Written by a sandboxed app, not downloaded.
        assert!(!quarantine_says_downloaded(
            b"0082;6ab9ae00;Microsoft Word;"
        ));
        assert!(!quarantine_says_downloaded(b"0086;6ab9ae00;Pages;"));
    }

    #[test]
    fn a_zone_stream_is_read_back_where_from_internet_writes_it() {
        let dir = std::env::temp_dir().join(format!("rata-mark-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Invoice.docx");
        std::fs::write(&file, b"PK").unwrap();
        assert!(!zone_marked(&file), "no stream");
        // Where Windows keeps the stream; beside the file anywhere else.
        let mut stream = file.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        std::fs::write(&stream, zone_identifier_bytes()).unwrap();
        assert!(zone_marked(&file));
        std::fs::write(&stream, b"[ZoneTransfer]\r\nZoneId=1\r\n").unwrap();
        assert!(!zone_marked(&file));
        // A test's reader stands in for the system's, on this thread only.
        {
            let _g = read_marks_with(|_| true);
            assert!(carries_mark(&file));
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        assert!(!carries_mark(&file), "Linux has no mark");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn what_from_internet_writes_is_read_back_as_a_mark() {
        let dir = std::env::temp_dir().join(format!("rata-mark-real-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Invoice.docx");
        std::fs::write(&file, b"PK").unwrap();
        assert!(!carries_mark(&file));
        from_internet(&file).unwrap();
        assert!(carries_mark(&file));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
