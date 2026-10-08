//! Copy Level / Paste Level go through the general pasteboard as plain text (platform.md P8).

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

/// Replaces the pasteboard's contents with `text`.
pub fn copy_text(text: &str) {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    // SAFETY: `NSPasteboardTypeString` is an immutable NSString constant.
    let kind = unsafe { NSPasteboardTypeString };
    pasteboard.setString_forType(&NSString::from_str(text), kind);
}

/// The pasteboard's plain text, if it has any.
pub fn paste_text() -> Option<String> {
    // SAFETY: as above.
    let kind = unsafe { NSPasteboardTypeString };
    NSPasteboard::generalPasteboard()
        .stringForType(kind)
        .map(|text| text.to_string())
}
