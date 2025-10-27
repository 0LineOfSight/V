use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/main.rs");
    let p = Path::new("src/main.rs");
    if let Ok(mut s) = fs::read_to_string(p) {
        if !s.contains("pub use node::Node;") {
            s = format!("pub use node::Node;\n{}", s);
            let _ = fs::write(p, s);
            println!("cargo:warning=Hotfix 22 injected `pub use node::Node;` into node/src/main.rs");
        }
    }
}
