use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    let path = Path::new("src/lib.rs");
    let Ok(mut src) = fs::read_to_string(path) else {
        // If the file isn't there, do nothing.
        return;
    };

    let mut changed = false;

    // 1) Fix the blake3 slice: remove the extra leading '&'
    let before = "&blake3::hash(data).as_bytes()";
    if src.contains(before) {
        src = src.replace(before, "blake3::hash(data).as_bytes()");
        changed = true;
    }

    // 2) Borrow checker fix: precompute has_payload before mutable entry() borrow
    let marker_line = "let e = rbc.ready.entry(root).or_default();";
    if !src.contains("__has_payload") {
        if let Some(pos) = src.find(marker_line) {
            // Insert a cached flag right before the entry() borrow.
            let inject = "let __has_payload = rbc.has_payload(&root);\n                    ";
            src.insert_str(pos, inject);
            changed = true;
        }
    }
    // Replace the condition to use the cached flag
    if src.contains("&& rbc.has_payload(&root)") {
        src = src.replace("&& rbc.has_payload(&root)", "&& __has_payload");
        changed = true;
    }
    if src.contains("&& rbc.has_payload(& root)") {
        src = src.replace("&& rbc.has_payload(& root)", "&& __has_payload");
        changed = true;
    }

    if changed {
        // Write back the patched file
        let _ = fs::write(path, src);
        // Ensure Cargo sees a fresh timestamp
        println!("cargo:warning=Applied Hotfix 15 consensus patches (blake3 slice & borrow ordering).");
    }
}
