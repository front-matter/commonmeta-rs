fn main() {
    for path in ["/opt/homebrew/lib", "/usr/local/lib"] {
        if std::path::Path::new(path).join("libduckdb.dylib").exists()
            || std::path::Path::new(path).join("libduckdb.a").exists()
        {
            println!("cargo:rustc-link-search=native={path}");
        }
    }
}
