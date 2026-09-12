use windows::core::w;
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

pub const AUMID: windows::core::PCWSTR = w!("FlyingSatoshi.Unseat");

/// Establish taskbar identity without copying the executable, spawning a child,
/// or creating shortcuts. Unseat runs from the location chosen by the user.
pub fn set_aumid() {
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(AUMID);
    }
}
