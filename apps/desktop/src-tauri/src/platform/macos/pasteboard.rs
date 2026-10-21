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
            // An item whose types gave no data has nothing to put back.
            if !entry.is_empty() {
                items.push(entry);
            }
        }
    }
    ClipboardSnapshot { items }
}

pub fn text() -> Option<String> {
    let pb = NSPasteboard::generalPasteboard();
    // SAFETY: `NSPasteboardTypeString` is a process-lifetime extern static;
    // objc2 requires unsafe to read it.
    let ty = unsafe { NSPasteboardTypeString };
    Some(pb.stringForType(ty)?.to_string())
}

/// Puts `text` on the pasteboard. `false` when the system refused the
/// write; the pasteboard is then empty.
pub fn write_text(text: &str) -> bool {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    // SAFETY: `NSPasteboardTypeString` is a process-lifetime extern static;
    // objc2 requires unsafe to read it.
    let ty = unsafe { NSPasteboardTypeString };
    let ok = pb.setString_forType(&NSString::from_str(text), ty);
    if !ok {
        tracing::warn!("the pasteboard refused the text");
    }
    ok
}

/// The pasteboard type by which clipboard histories recognise content that
/// is about to be replaced again (the nspasteboard.org convention).
const TRANSIENT_TYPE: &str = "org.nspasteboard.TransientType";

pub fn write_passing(text: &str) -> bool {
    if !write_text(text) {
        return false;
    }
    let pb = NSPasteboard::generalPasteboard();
    pb.setData_forType(Some(&NSData::new()), &NSString::from_str(TRANSIENT_TYPE));
    true
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
