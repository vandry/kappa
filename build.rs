use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("$OUT_DIR"));
    let js_dir = out_dir.join("node_modules");
    let _ = std::fs::create_dir(&js_dir);
    let js_dir_str = js_dir.to_str().unwrap();

    println!("cargo:rerun-if-changed=browser");
    std::fs::copy("browser/package.json", out_dir.join("package.json"))?;
    let ok = std::process::Command::new("npm")
        .arg("install")
        .current_dir(&out_dir)
        .status()?
        .success();
    if !ok {
        return Err("npm install failed".into());
    }

    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("fdset.bin"))
        .protoc_arg(format!(
            "--plugin=protoc-gen-js={js_dir_str}/protoc-gen-js/bin/protoc-gen-js"
        ))
        .protoc_arg(format!(
            "--plugin=protoc-gen-grpc-web={js_dir_str}/protoc-gen-grpc-web/bin/protoc-gen-grpc-web"
        ))
        .protoc_arg(format!("--js_out=import_style=commonjs:{js_dir_str}"))
        .protoc_arg(format!(
            "--grpc-web_out=import_style=commonjs,mode=grpcwebtext:{js_dir_str}"
        ))
        .compile_protos(&["proto/browser.proto", "proto/gateway.proto"], &["proto"])?;
    let ok = std::process::Command::new("npx")
        .arg("webpack")
        .arg(OsString::from_vec(
            [b"--env=out_dir=", js_dir.as_os_str().as_bytes()].concat(),
        ))
        .current_dir("browser")
        .status()?
        .success();
    if !ok {
        return Err("npx webpack failed".into());
    }

    Ok(())
}
