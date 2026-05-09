fn main() {
    #[cfg(target_os = "windows")]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let out_dir = std::env::var("OUT_DIR").unwrap();

        // Use absolute path to avoid windres CWD-relative lookup failure
        let ico_abs = std::path::Path::new(&manifest_dir)
            .join("assets")
            .join("app.ico");
        // RC paths must use forward slashes to avoid RC escape issues
        let ico_str = ico_abs.to_string_lossy().replace('\\', "/");

        let rc_path = std::path::Path::new(&out_dir).join("app_icon.rc");
        let obj_path = std::path::Path::new(&out_dir).join("app_icon.o");

        // Write application manifest (Common Controls v6 required by TaskDialogIndirect)
        let manifest_path = std::path::Path::new(&out_dir).join("app.manifest");
        let manifest_abs = manifest_path.to_string_lossy().replace('\\', "/");
        std::fs::write(
            &manifest_path,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0" processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <!-- DPI Awareness: PerMonitorV2 fixes tray menu position offset on Win10 LTSC 2019.
       Without this, the process is DPI-unaware and GetCursorPos returns virtualized
       coordinates that diverge from TrackPopupMenu's physical rendering when
       explorer's DPI-aware thread input is temporarily attached. -->
  <asmv3:application xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
    <asmv3:windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/PM</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
    </asmv3:windowsSettings>
  </asmv3:application>
</assembly>
"#,
        )
        .expect("Failed to write app.manifest");

        // Write RC file: icon + manifest (RT_MANIFEST = 24, CREATEPROCESS_MANIFEST_RESOURCE_ID = 1)
        std::fs::write(
            &rc_path,
            format!("1 ICON \"{}\"\r\n1 24 \"{}\"\r\n", ico_str, manifest_abs),
        )
        .expect("Failed to write app_icon.rc");

        // Compile RC -> .o using windres (MinGW); passes --output-format=coff
        // so GNU ld can link the PE resource section directly
        let status = std::process::Command::new("windres")
            .arg(rc_path.to_str().unwrap())
            .arg("--output-format=coff")
            .arg("-o")
            .arg(obj_path.to_str().unwrap())
            .status()
            .expect("windres not found – ensure MinGW/bin is in PATH");

        assert!(status.success(), "windres failed to compile app_icon.rc");

        // Link the .o DIRECTLY (not via .a archive).
        // winres uses -l static=resource which requires symbol references;
        // GNU ld silently drops .rsrc sections with no exported symbols,
        // so the EXE ends up with no icon. Passing the .o via link-arg bypasses
        // that stripping and preserves the .rsrc section.
        println!("cargo:rustc-link-arg={}", obj_path.display());

        // Re-run this script only when the icon changes
        println!("cargo:rerun-if-changed=assets/app.ico");
    }
}
