// Windows: the program's version info and icon. Girovo's consent dialog shows
// the signature's program name ("Girovo CLI", scripts/sign.ps1 sets it), else
// FileDescription from here, and the icon from the executable.

fn main() {
  // Built on Windows for Windows only (cross builds for Linux skip it).
  #[cfg(windows)]
  if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
    let mut res = winresource::WindowsResource::new();
    res
      .set_icon("assets/girovo-cli.ico")
      .set("FileDescription", "Girovo CLI")
      .set("ProductName", "Girovo CLI")
      .set("CompanyName", "MT-Media")
      .set("LegalCopyright", "© MT-Media")
      .set("OriginalFilename", "girovo-cli.exe")
      .set("InternalName", "girovo-cli");
    if let Err(e) = res.compile() {
      panic!("Windows resources: {e}");
    }
  }
  println!("cargo:rerun-if-changed=assets/girovo-cli.ico");
}
