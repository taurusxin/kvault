fn main() {
    println!("cargo:rerun-if-changed=src/security/macos.swift");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        use std::{path::PathBuf, process::Command};
        let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
        let object = output.join("kvault_auth.o");
        let archive = output.join("libkvault_auth.a");
        let arch = if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        let compiler = Command::new("xcrun")
            .args(["--find", "swiftc"])
            .output()
            .expect("Xcode command line tools are required");
        assert!(compiler.status.success(), "swiftc is unavailable");
        let compiler = PathBuf::from(
            String::from_utf8(compiler.stdout)
                .expect("swiftc path")
                .trim(),
        );
        let status = Command::new("xcrun")
            .args([
                "--sdk",
                "macosx",
                "swiftc",
                "-parse-as-library",
                "-emit-object",
                "-O",
                "-module-name",
                "KVaultDeviceSecurity",
                "-target",
                &format!("{arch}-apple-macosx11.0"),
                "-module-cache-path",
            ])
            .arg(output.join("swift-cache"))
            .arg("src/security/macos.swift")
            .arg("-o")
            .arg(&object)
            .status()
            .expect("compile native system authentication");
        assert!(
            status.success(),
            "Swift system authentication compilation failed"
        );
        assert!(
            Command::new("xcrun")
                .arg("ar")
                .arg("crs")
                .arg(&archive)
                .arg(&object)
                .status()
                .expect("archive Swift bridge")
                .success()
        );
        println!("cargo:rustc-link-search=native={}", output.display());
        println!("cargo:rustc-link-lib=static=kvault_auth");
        let runtime = compiler
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("lib/swift/macosx");
        println!("cargo:rustc-link-search=native={}", runtime.display());
        println!("cargo:rustc-link-search=native=/usr/lib/swift");
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        for framework in ["Foundation", "Security", "LocalAuthentication", "CryptoKit"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
    tauri_build::build()
}
