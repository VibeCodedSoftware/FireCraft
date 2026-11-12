//! Windows builds carry FireCraft's icon (the web app's, packaging/windows/firecraft.ico) in the
//! executable, so Explorer, the taskbar and launchers show it.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/firecraft.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../packaging/windows/firecraft.ico");
    res.set("ProductName", "FireCraft");
    res.set("FileDescription", "FireCraft");
    if let Err(e) = res.compile() {
        println!("cargo:warning=FireCraft's icon wasn't embedded: {e}");
    }
}
