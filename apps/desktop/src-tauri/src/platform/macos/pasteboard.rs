//! Snapshot, write, and restore of the general pasteboard. The snapshot
//! keeps every item and every type so a paste round trip does not lose
//! files, rich text, or images the user had copied.

use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString};

use crate::platform::ClipboardSnapshot;

pub fn snapshot() -> ClipboardSnapshot {
    let pb = NSPasteboard::generalPasteboard();
    let mut items = Vec::new();
    if let Some(list) = pb.pasteboardItems() {
        for i in 0..list.count() {
            let item = list.objectAtIndex(i);
            let mut entry = Vec::new();
            let types = item.types();
            for t in 0..types.count() {
                let ty = types.objectAtIndex(t);
                if let Some(data) = item.dataForType(&ty) {
                    entry.push((ty.to_string(), data.to_vec()));
                }
            }
            items.push(entry);
        }
    }
    ClipboardSnapshot { items }
}

pub fn text() -> Option<String> {
    let pb = NSPasteboard::generalPasteboard();
    // SAFETY: `NSPasteboardTypeString` is a process-lifetime extern static;
    // objc2 requires unsafe to read it, and the call has no other
    // precondition.
    let s = unsafe { pb.stringForType(NSPasteboardTypeString) }?;
    Some(s.to_string())
}

pub fn write_text(text: &str) {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    // SAFETY: `NSPasteboardTypeString` is a valid extern static; the
    // `NSString` argument is owned by this call and kept alive for its
    // duration.
    unsafe {
        pb.setString_forType(&NSString::from_str(text), NSPasteboardTypeString);
    }
}

/// The pasteboard type by which clipboard histories recognise content that
/// is about to be replaced again (the nspasteboard.org convention).
const TRANSIENT_TYPE: &str = "org.nspasteboard.TransientType";

pub fn write_passing(text: &str) {
    write_text(text);
    let pb = NSPasteboard::generalPasteboard();
    pb.setData_forType(Some(&NSData::new()), &NSString::from_str(TRANSIENT_TYPE));
}

pub fn restore(snapshot: ClipboardSnapshot) {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    if snapshot.items.is_empty() {
        return;
    }
    let mut objects = Vec::new();
    for entry in snapshot.items {
        let item = NSPasteboardItem::new();
        for (ty, bytes) in entry {
            let ok = item.setData_forType(&NSData::with_bytes(&bytes), &NSString::from_str(&ty));
            if !ok {
                tracing::debug!(%ty, "pasteboard type could not be restored");
            }
        }
        objects.push(ProtocolObject::from_retained(item));
    }
    let array = NSArray::from_retained_slice(&objects);
    let array: &NSArray<ProtocolObject<dyn NSPasteboardWriting>> = &array;
    if !pb.writeObjects(array) {
        tracing::warn!("pasteboard restore failed");
    }
}
