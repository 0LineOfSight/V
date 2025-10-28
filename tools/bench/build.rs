use std::fs;

fn read(path: &str) -> Option<String> { fs::read_to_string(path).ok() }
fn write(path: &str, s: &str) { let _ = fs::write(path, s); }

fn main() {
    println!("cargo:rerun-if-changed=src/main.rs");
    if let Some(mut s) = read("src/main.rs") {
        let mut changed = false;
        // Fix Python-style format specifier -> Rust style
        if s.contains(":.2f") {
            s = s.replace(":.2f", ":.2");
            println!("cargo:warning=bench: replaced `:.2f` with `:.2` in format strings");
            changed = true;
        }
        // Remove http2_prior_knowledge() which is not available in reqwest 0.12.x
        if s.contains(".http2_prior_knowledge()") {
            s = s.replace(".http2_prior_knowledge()", "");
            println!("cargo:warning=bench: removed `.http2_prior_knowledge()`");
            changed = true;
        }
        // Remove unused imports: Duration and tracing::info
        if s.contains("use std::time::{Duration, Instant};") {
            s = s.replace("use std::time::{Duration, Instant};", "use std::time::Instant;");
            println!("cargo:warning=bench: removed unused `Duration` import");
            changed = true;
        }
        if s.contains("use tracing::info;") {
            s = s.replace("use tracing::info;", "");
            println!("cargo:warning=bench: removed unused `tracing::info` import");
            changed = true;
        }
        if changed {
            write("src/main.rs", &s);
        }
    }
}
