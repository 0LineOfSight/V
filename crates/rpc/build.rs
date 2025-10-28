use std::fs;

fn read(path: &str) -> Option<String> { fs::read_to_string(path).ok() }
fn write(path: &str, s: &str) { let _ = fs::write(path, s); }

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    if let Some(mut s) = read("src/lib.rs") {
        let before = s.clone();

        // Update legacy :param to {param} for axum >= 0.8
        if s.contains(r#""/balance/:addr""#) {
            s = s.replace(r#""/balance/:addr""#, r#""/balance/{addr}""#);
            // Print a cargo warning (avoid formatting braces by using a single placeholder)
            println!("cargo:warning={}", "rpc hotfix: changed \"/balance/:addr\" to \"/balance/{addr}\"");
        }

        if s != before {
            write("src/lib.rs", &s);
        }
    }
}
