//! Embeds the icon, version details and the Windows manifest (administrator rights, DPI awareness)
//! into PerifeMngr.exe.

const APPLICATION_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="NikiCoding.PerifeMngr" version="1.0.0.0"/>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if is_release_build() {
        embed_administrator_manifest();
    }
    embed_icon_and_version_details();
}

/// Debug builds and `cargo test` stay unelevated so they can run from a normal terminal.
fn is_release_build() -> bool {
    std::env::var("PROFILE").is_ok_and(|profile| profile == "release")
}

fn embed_administrator_manifest() {
    let output_directory = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let manifest_path = std::path::Path::new(&output_directory).join("PerifeMngr.exe.manifest");
    std::fs::write(&manifest_path, APPLICATION_MANIFEST).expect("manifest is written");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:NO");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest_path.display());
}

fn embed_icon_and_version_details() {
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/app.ico")
        .set("ProductName", "PerifeMngr")
        .set("FileDescription", "PerifeMngr")
        .set("CompanyName", "NikiCoding")
        .set("LegalCopyright", "Copyright © 2026 NikiCoding")
        .set("OriginalFilename", "PerifeMngr.exe");
    resource.compile().expect("Windows resources compile");
}
