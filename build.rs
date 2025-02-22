#[cfg(target_os = "windows")]
fn main() {
    extern crate windows_exe_info;
    windows_exe_info::icon::icon_ico(std::path::Path::new("./images/logo.ico"));
}

#[cfg(not(target_os = "windows"))]
fn main() {}
