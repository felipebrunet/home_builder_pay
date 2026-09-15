fn main() {
    #[cfg(target_os = "linux")]
    {
        let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("native/linux");
        if dir.join("libxdo.so").exists() || dir.join("libxdo.a").exists() {
            println!("cargo:rustc-link-search=native={}", dir.display());
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
        }
        println!("cargo:rerun-if-changed=native/linux");
    }
}
