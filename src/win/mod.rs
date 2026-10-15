//! The Win32 host. Everything here runs on the UI thread (the thread in `app::run`), except
//! the iPhone listener and sender threads in `phone` and the background checks in `update` and
//! `phone::check_tailscale`. Those never touch `App` or a window's state: they share data
//! behind mutexes and only post messages or invalidate windows.
//!
//! # Unsafe code
//!
//! Most `unsafe` here is the Win32 and COM bindings themselves: arguments are owned values or
//! locals that outlive the call, and buffers Windows keeps using afterwards (the scaled WAV
//! behind `PlaySoundW(SND_ASYNC)`) stay alive until it stops. The rest follows one model:
//!
//! - `App` is boxed in `run`, and its pointer is stored in the user data of the hidden and
//!   overlay windows and handed to popups at creation. It is used only on the UI thread while
//!   the message loop runs. At exit the popups are destroyed and both windows' user data is
//!   cleared before the box is freed.
//! - Each popup (settings, tray panel, hover card, time-up tabs) boxes its own state in its
//!   window's user data in `WM_CREATE` and frees it in `WM_DESTROY`.
//! - Re-entrancy is the one real hazard. Calls such as `ShowWindow`, `SetWindowPos` or
//!   `SendMessageW` deliver messages synchronously, so a window procedure can run while an
//!   `App` method is still on the stack, and it then works on the same `App` through a
//!   second `&mut`. Keep methods short, and don't hold references into `App` across a call
//!   that may dispatch messages.

mod app;
mod autostart;
mod docking;
mod flyout;
mod gfx;
mod handoff;
mod hotkeys;
mod icon;
mod idle;
mod install;
mod paint;
mod phone;
mod prompt;
mod session;
mod settings_dlg;
mod sound;
mod testhook;
mod theme;
mod tray;
mod tray_panel;
mod update;
mod widget;
mod winhttp;

pub use app::run;
