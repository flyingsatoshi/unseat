use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use unseat::{installed_exe, is_dev_build};
use windows::Win32::Foundation::{HANDLE, TRUE};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    IShellLinkW, SetCurrentProcessExplicitAppUserModelID, SHGetKnownFolderPath, FOLDERID_Programs,
    KF_FLAG_DEFAULT, ShellLink,
};
use windows::core::{w, Interface, PCWSTR};

pub const AUMID: windows::core::PCWSTR = w!("FlyingSatoshi.Unseat");

pub fn prepare() -> bool {
    set_aumid();
    match relocate_target() {
        Some(dest) if Command::new(&dest).spawn().is_ok() => true,
        _ => false,
    }
}

pub fn set_aumid() {
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(AUMID);
    }
}

fn relocate_target() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
    if is_dev_build(&current) {
        return None;
    }
    let local = std::env::var_os("LOCALAPPDATA")?;
    let dest = installed_exe(Path::new(&local));
    if let (Ok(a), Ok(b)) = (current.canonicalize(), dest.canonicalize()) {
        if a == b {
            let _ = write_start_menu_shortcut(&dest);
            return None;
        }
    }
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::copy(&current, &dest);
    if !dest.is_file() {
        return None;
    }
    let _ = write_start_menu_shortcut(&dest);
    Some(dest)
}

fn write_start_menu_shortcut(exe: &Path) -> windows::core::Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let programs = SHGetKnownFolderPath(&FOLDERID_Programs, KF_FLAG_DEFAULT, HANDLE::default())?;
        let dir = programs.to_string()?;
        CoTaskMemFree(Some(programs.0 as _));
        let link_path = PathBuf::from(dir).join("Unseat.lnk");
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let exe_w = wide(exe);
        let dir_w = wide(exe.parent().unwrap_or(exe));
        link.SetPath(PCWSTR(exe_w.as_ptr()))?;
        link.SetWorkingDirectory(PCWSTR(dir_w.as_ptr()))?;
        link.SetIconLocation(PCWSTR(exe_w.as_ptr()), 0)?;
        link.SetDescription(w!("Unseat break reminder"))?;
        let persist: IPersistFile = link.cast()?;
        let link_w = wide(&link_path);
        persist.Save(PCWSTR(link_w.as_ptr()), TRUE)?;
        windows::Win32::UI::Shell::SHChangeNotify(
            windows::Win32::UI::Shell::SHCNE_ASSOCCHANGED,
            windows::Win32::UI::Shell::SHCNF_IDLIST,
            None,
            None,
        );
        Ok(())
    }
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
