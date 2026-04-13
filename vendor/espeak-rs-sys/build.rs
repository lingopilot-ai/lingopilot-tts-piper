use cmake::Config;
use glob::glob;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

macro_rules! debug_log {
    ($($arg:tt)*) => {
        if std::env::var("BUILD_DEBUG").is_ok() {
            println!("cargo:warning=[DEBUG] {}", format!($($arg)*));
        }
    };
}

fn get_cargo_target_dir() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let profile = std::env::var("PROFILE")?;
    let mut target_dir = None;
    let mut sub_path = out_dir.as_path();
    while let Some(parent) = sub_path.parent() {
        if parent.ends_with(&profile) {
            target_dir = Some(parent);
            break;
        }
        sub_path = parent;
    }
    let target_dir = target_dir.ok_or("not found")?;
    Ok(target_dir.to_path_buf())
}

fn copy_folder(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("Failed to create dst directory");
    if cfg!(unix) {
        let status = std::process::Command::new("cp")
            .arg("-rf")
            .arg(src)
            .arg(dst.parent().unwrap())
            .status()
            .expect("Failed to execute cp command");
        assert!(status.success(), "cp command failed with {:?}", status);
    }

    if cfg!(windows) {
        let status = std::process::Command::new("robocopy.exe")
            .arg("/e")
            .arg(src)
            .arg(dst)
            .status()
            .expect("Failed to execute robocopy command");
        // robocopy exit codes: 0-7 = success (various copy states), >=8 = error
        let code = status.code().unwrap_or(8);
        assert!(code < 8, "robocopy failed with exit code {}", code);
    }
}

fn copy_directory_recursive(src: &Path, dst: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = dst.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory_recursive(&source_path, &target_path)?;
        } else {
            std::fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn espeak_runtime_layout_ready(root_dir: &Path) -> bool {
    let data_dir = root_dir.join("espeak-ng-data");
    data_dir.join("phontab").is_file()
        && data_dir.join("phondata").is_file()
        && data_dir.join("phonindex").is_file()
        && data_dir.join("en_dict").is_file()
        && data_dir.join("voices").is_dir()
}

fn publish_espeak_runtime_assets(
    out_dir: &Path,
    target_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let source_root = [out_dir.join("share"), out_dir.join("build")]
        .into_iter()
        .find(|candidate| espeak_runtime_layout_ready(candidate))
        .ok_or_else(|| {
            format!(
                "eSpeak build did not produce compiled runtime assets under '{}' or '{}'",
                out_dir.join("share").display(),
                out_dir.join("build").display()
            )
        })?;

    let runtime_root = target_dir.join("espeak-runtime");
    if runtime_root.exists() {
        std::fs::remove_dir_all(&runtime_root)?;
    }

    copy_directory_recursive(
        &source_root.join("espeak-ng-data"),
        &runtime_root.join("espeak-ng-data"),
    )?;
    debug_log!(
        "Published eSpeak runtime from {} to {}",
        source_root.display(),
        runtime_root.display()
    );
    Ok(())
}

fn extract_lib_names(out_dir: &Path, build_shared_libs: bool) -> Vec<String> {
    let lib_pattern = if cfg!(windows) {
        "*.lib"
    } else if cfg!(target_os = "macos") {
        if build_shared_libs {
            "*.dylib"
        } else {
            "*.a"
        }
    } else {
        if build_shared_libs {
            "*.so"
        } else {
            "*.a"
        }
    };
    let libs_dir = out_dir.join("lib");
    let pattern = libs_dir.join(lib_pattern);
    debug_log!("Extract libs {}", pattern.display());

    let mut lib_names: Vec<String> = Vec::new();

    // Process the libraries based on the pattern
    for entry in glob(pattern.to_str().unwrap()).unwrap() {
        match entry {
            Ok(path) => {
                let stem = path.file_stem().unwrap();
                let stem_str = stem.to_str().unwrap();

                // Remove the "lib" prefix if present
                let lib_name = if stem_str.starts_with("lib") {
                    stem_str.strip_prefix("lib").unwrap_or(stem_str)
                } else {
                    stem_str
                };
                lib_names.push(lib_name.to_string());
            }
            Err(e) => println!("cargo:warning=error={}", e),
        }
    }
    lib_names
}

fn extract_lib_assets(out_dir: &Path) -> Vec<PathBuf> {
    let shared_lib_pattern = if cfg!(windows) {
        "*.dll"
    } else if cfg!(target_os = "macos") {
        "*.dylib"
    } else {
        "*.so"
    };

    let libs_dir = out_dir.join("lib");
    let pattern = libs_dir.join(shared_lib_pattern);
    debug_log!("Extract lib assets {}", pattern.display());
    let mut files = Vec::new();

    for entry in glob(pattern.to_str().unwrap()).unwrap() {
        match entry {
            Ok(path) => {
                files.push(path);
            }
            Err(e) => eprintln!("cargo:warning=error={}", e),
        }
    }

    files
}

fn macos_link_search_path() -> Option<String> {
    let output = Command::new("clang")
        .arg("--print-search-dirs")
        .output()
        .ok()?;
    if !output.status.success() {
        println!(
            "failed to run 'clang --print-search-dirs', continuing without a link search path"
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("libraries: =") {
            let path = line.split('=').nth(1)?;
            return Some(format!("{}/lib/darwin", path));
        }
    }

    println!("failed to determine link search path, continuing without it");
    None
}

fn main() {
    println!("cargo:rerun-if-env-changed=ESPEAK_BUILD_SHARED_LIBS");
    println!("cargo:rerun-if-env-changed=ESPEAK_LIB_PROFILE");
    println!("cargo:rerun-if-env-changed=ESPEAK_STATIC_CRT");
    println!("cargo:rerun-if-env-changed=BUILD_DEBUG");
    println!("cargo:rustc-link-lib=speechPlayer");
    println!("cargo:rustc-link-lib=espeak-ng");
    println!("cargo:rustc-link-lib=ucd");
    let target = env::var("TARGET").unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let target_dir = get_cargo_target_dir().unwrap();
    let espeak_dst = out_dir.join("espeak-ng");
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("Failed to get CARGO_MANIFEST_DIR");
    let espeak_src = Path::new(&manifest_dir).join("espeak-ng");
    let build_shared_libs = false;

    let build_shared_libs = std::env::var("ESPEAK_BUILD_SHARED_LIBS")
        .map(|v| v == "1")
        .unwrap_or(build_shared_libs);
    let profile = env::var("ESPEAK_LIB_PROFILE").unwrap_or("Release".to_string());
    let static_crt = env::var("ESPEAK_STATIC_CRT")
        .map(|v| v == "1")
        .unwrap_or(false);

    debug_log!("TARGET: {}", target);
    debug_log!("CARGO_MANIFEST_DIR: {}", manifest_dir);
    debug_log!("TARGET_DIR: {}", target_dir.display());
    debug_log!("OUT_DIR: {}", out_dir.display());
    debug_log!("BUILD_SHARED: {}", build_shared_libs);
    debug_log!("PROFILE: {}", profile);
    debug_log!("STATIC_CRT: {}", static_crt);

    // Prepare espeak-ng source
    if !espeak_dst.exists() {
        debug_log!("Copy {} to {}", espeak_src.display(), espeak_dst.display());
        copy_folder(&espeak_src, &espeak_dst);
    }
    // Speed up build
    env::set_var(
        "CMAKE_BUILD_PARALLEL_LEVEL",
        std::thread::available_parallelism()
            .unwrap()
            .get()
            .to_string(),
    );

    // Bindings
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", espeak_dst.display()))
        .clang_arg(format!(
            "-I{}",
            espeak_dst.join("src").join("include").display()
        ))
        .clang_arg(format!(
            "-I{}",
            espeak_src.join("src").join("include").display()
        ))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Failed to generate bindings");

    // Write the generated bindings to an output file
    let bindings_path = out_dir.join("bindings.rs");
    bindings
        .write_to_file(bindings_path)
        .expect("Failed to write bindings");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=./espeak-ng");

    debug_log!("Bindings Created");

    // Build with Cmake

    let mut config = Config::new(&espeak_dst);

    config.define(
        "BUILD_SHARED_LIBS",
        if build_shared_libs { "ON" } else { "OFF" },
    );

    if cfg!(windows) {
        config.static_crt(static_crt);
    }

    if cfg!(target_os = "macos") {
        config.define("USE_LIBPCAUDIO", "OFF");
    }

    // General
    config
        .profile(&profile)
        .define("ENABLE_TESTS", "OFF")
        // eSpeak-ng's "data" target produces the runtime files Piper needs
        // (`phontab`, `phondata`, dictionaries). Without them the library
        // initializes but phonemization fails at runtime.
        .define("COMPILE_INTONATIONS", "ON")
        .very_verbose(std::env::var("CMAKE_VERBOSE").is_ok()) // Not verbose by default
        // Windows builds depend on environment-selected CRT/profile options.
        // Reconfigure every run so stale CMake cache entries do not keep a prior
        // Debug build alive after switching back to the Release CRT path.
        .always_configure(cfg!(windows));

    let bindings_dir = config.build();
    publish_espeak_runtime_assets(&out_dir, &target_dir)
        .expect("Failed to publish compiled eSpeak runtime assets");

    // Search paths
    println!("cargo:rustc-link-search={}", out_dir.join("lib").display());
    println!(
        "cargo:rustc-link-search={}",
        out_dir.join("build/src/speechPlayer").display()
    );
    println!(
        "cargo:rustc-link-search={}",
        out_dir.join("build/src/ucd-tools").display()
    );
    println!("cargo:rustc-link-search={}", bindings_dir.display());

    if cfg!(windows) {
        println!(
            "cargo:rustc-link-search={}",
            out_dir.join("build/src/speechPlayer/Release").display()
        );
        println!(
            "cargo:rustc-link-search={}",
            out_dir.join("build/src/ucd-tools/Release").display()
        );
    }

    // macOS
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=c++");
    }

    // Link libraries
    let espeak_libs_kind = if build_shared_libs { "dylib" } else { "static" };
    let espeak_libs = extract_lib_names(&out_dir, build_shared_libs);

    for lib in espeak_libs {
        debug_log!(
            "LINK {}",
            format!("cargo:rustc-link-lib={}={}", espeak_libs_kind, lib)
        );
        println!(
            "{}",
            format!("cargo:rustc-link-lib={}={}", espeak_libs_kind, lib)
        );
    }

    // Windows debug — removed explicit msvcrtd link.
    // Linking the legacy msvcrtd.dll alongside the modern UCRT (ucrtbased.dll)
    // creates two separate CRT file handle tables, causing _osfile(fh) & FOPEN
    // debug assertions when other C libraries (e.g. CTranslate2) share the process.
    // The Rust toolchain already links the correct CRT automatically.

    // Linux
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    if target.contains("apple") {
        // On (older) OSX we need to link against the clang runtime,
        // which is hidden in some non-default path.
        //
        // More details at https://github.com/alexcrichton/curl-rust/issues/279.
        if let Some(path) = macos_link_search_path() {
            println!("cargo:rustc-link-lib=clang_rt.osx");
            println!("cargo:rustc-link-search={}", path);
        }
    }

    // copy DLLs to target
    if build_shared_libs {
        let libs_assets = extract_lib_assets(&out_dir);
        for asset in libs_assets {
            let asset_clone = asset.clone();
            let filename = asset_clone.file_name().unwrap();
            let filename = filename.to_str().unwrap();
            let dst = target_dir.join(filename);
            debug_log!("HARD LINK {} TO {}", asset.display(), dst.display());
            if !dst.exists() {
                std::fs::hard_link(asset.clone(), dst).unwrap();
            }

            // Copy DLLs to examples as well
            if target_dir.join("examples").exists() {
                let dst = target_dir.join("examples").join(filename);
                debug_log!("HARD LINK {} TO {}", asset.display(), dst.display());
                if !dst.exists() {
                    std::fs::hard_link(asset.clone(), dst).unwrap();
                }
            }

            // Copy DLLs to target/profile/deps as well for tests
            let dst = target_dir.join("deps").join(filename);
            debug_log!("HARD LINK {} TO {}", asset.display(), dst.display());
            if !dst.exists() {
                std::fs::hard_link(asset.clone(), dst).unwrap();
            }
        }
    }
}
