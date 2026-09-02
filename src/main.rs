#![windows_subsystem = "windows"]

mod win;

fn main() {
    if let Err(err) = win::run() {
        let _ = err;
    }
}
