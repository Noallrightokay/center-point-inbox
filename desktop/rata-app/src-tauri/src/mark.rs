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
}
