use std::fs;

fn read(path: &str) -> Option<String> { std::fs::read_to_string(path).ok() }
fn write(path: &str, s: &str) { let _ = std::fs::write(path, s); }

fn fix_adapter_path_in_main() {
    if let Some(mut s) = read("src/main.rs") {
        let before = s.clone();
        // Replace any occurrence of crate::NodeApiAdapter(...) with node::NodeApiAdapter(...)
        s = s.replace("crate::NodeApiAdapter(", "node::NodeApiAdapter(");
        if s != before {
            write("src/main.rs", &s);
            println!("cargo:warning=Hotfix 33: main.rs now uses node::NodeApiAdapter(..) for rpc::serve");
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=src/main.rs");
    fix_adapter_path_in_main();
}
