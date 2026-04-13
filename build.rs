fn main() {
    // eSpeak-NG uses Windows registry APIs (RegOpenKeyExA, RegQueryValueExA)
    // which live in advapi32.lib
    if cfg!(target_os = "windows") {
        println!("cargo:rustc-link-lib=advapi32");
    }
}
