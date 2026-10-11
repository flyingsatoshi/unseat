pub mod about;
mod alerts;
mod audio;
pub mod companion;
mod date;
mod dock;
mod engine;
mod place;
mod settings;
mod stats;
mod theme;
mod view;

pub use alerts::{alert_actions, AlertActions, Notice};
pub use audio::{scale_wav, volume_peak, wav_peak, MAX_PEAK};
pub use date::CivilDate;
pub use dock::{
    dock_frame, edge_offset, nearest_edge, DockController, DockDraw, DockEdge, DockFrame, DockPhase,
    DockReveal, Rect, DOCK_SNAP, PILL_MARGIN, TAB_LEN, TAB_THICK,
};
pub use engine::{
    clamp_clock, next_tick_delay, restored_today_sitting, Beep, Command, EngineConfig, Input, SittingEngine,
    Snapshot, TickResult, VisibleState,
};
pub use place::{is_dev_build, newer_build_takes_over};
pub use settings::{
    AlertSound, DayTotal, InactivityBehavior, PhoneAlerts, Settings, ThemePreference, TimerCheckpoint,
    TimerDisplayMode, TimerShape, WidgetSize, ALERT_DURATION_PRESETS_SECS, DEFAULT_ALERT_VOLUME,
    HISTORY_DAYS, SETTINGS_VERSION, SNOOZE_PRESETS_SECS,
};
pub use stats::{average_secs, recent_days, weekday_initial, DayBar};
pub use theme::{contrast, palette, OverlayColors, Palette, Rgba, DARK, LIGHT, WARN_AT};
pub use view::{
    chip_center, face_digits, format_elapsed, format_goal_parts, format_limit, format_today, format_today_hm,
    hit_control, hit_overlay, layout_scale, progress, snap_to_edges, status_line, tag_text, timer_digit_px,
    timer_face_digits, timer_render_key, widget_layout, widget_pixel_size, Hit, TimerRenderKey,
    WidgetLayout, PILL_RADIUS, WIDGET_H, WIDGET_W,
};
