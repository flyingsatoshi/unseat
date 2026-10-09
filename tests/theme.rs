use unseat::{
    average_secs, contrast, palette, recent_days, weekday_initial, CivilDate, DayTotal, Palette,
    Rgba, VisibleState, DARK, LIGHT,
};

fn both() -> [&'static Palette; 2] {
    [&DARK, &LIGHT]
}

#[test]
fn settings_text_meets_wcag_aa_on_cards_and_window() {
    for p in both() {
        for bg in [p.card, p.window] {
            assert!(contrast(p.text, bg) >= 7.0, "text dark={}", p.dark);
            assert!(contrast(p.text_secondary, bg) >= 4.5, "secondary dark={}", p.dark);
        }
        assert!(contrast(p.text, p.control) >= 4.5, "chip text dark={}", p.dark);
        assert!(contrast(p.on_accent, p.accent) >= 4.5, "accent text dark={}", p.dark);
    }
}

#[test]
fn focus_ring_and_toggles_are_visible_against_their_surroundings() {
    for p in both() {
        assert!(contrast(p.focus, p.card) >= 3.0, "focus dark={}", p.dark);
        assert!(contrast(p.focus, p.window) >= 3.0, "focus on window dark={}", p.dark);
        assert!(contrast(p.accent, p.card) >= 3.0, "toggle on dark={}", p.dark);
    }
}

#[test]
fn overlay_digits_are_high_contrast_in_every_state() {
    for p in both() {
        for state in [VisibleState::Sitting, VisibleState::Overdue] {
            let bg = p.pill_for(state).over(if p.dark { Rgba::opaque(0) } else { Rgba::opaque(0xFFFFFF) });
            assert!(contrast(p.digits, bg) >= 12.0, "digits dark={} {state:?}", p.dark);
            assert!(contrast(p.tag.over(bg), bg) >= 4.5, "tag dark={} {state:?}", p.dark);
        }
    }
}

#[test]
fn bar_colors_separate_states() {
    let p = palette(true);
    assert_eq!(p.bar_for(VisibleState::Sitting, 0.2), p.bar_normal);
    assert_eq!(p.bar_for(VisibleState::Sitting, 0.9), p.bar_warn);
    assert_eq!(p.bar_for(VisibleState::Overdue, 1.0), p.bar_over);
    assert_eq!(p.bar_for(VisibleState::Break, 0.5), p.bar_break);
    assert_eq!(p.bar_for(VisibleState::Away, 0.5), p.bar_away);
    assert!(!palette(false).dark);
}

#[test]
fn dark_overlay_keeps_the_established_surfaces() {
    assert_eq!(DARK.pill_for(VisibleState::Sitting).0, 0x10);
    assert_eq!(DARK.pill_for(VisibleState::Overdue), Rgba(0x2A, 0x10, 0x16, 248));
}

fn d(year: i32, month: u8, day: u8) -> CivilDate {
    CivilDate { year, month, day }
}

#[test]
fn civil_date_arithmetic_handles_month_year_and_leap_boundaries() {
    assert_eq!(d(2026, 3, 1).add_days(-1), d(2026, 2, 28));
    assert_eq!(d(2028, 3, 1).add_days(-1), d(2028, 2, 29));
    assert_eq!(d(2026, 12, 31).add_days(1), d(2027, 1, 1));
    assert_eq!(d(1970, 1, 1).days_since_epoch(), 0);
    assert_eq!(d(2026, 10, 9).weekday(), 4); // Friday
    assert_eq!(weekday_initial(d(2026, 10, 5)), "M");
}

#[test]
fn recent_days_fills_gaps_and_uses_the_live_today_total() {
    let history = vec![
        DayTotal { date: "2026-10-07".into(), sitting_secs: 3600 },
        DayTotal { date: "2026-10-08".into(), sitting_secs: 7200 },
        DayTotal { date: "2026-09-01".into(), sitting_secs: 99 },
    ];
    let bars = recent_days(&history, d(2026, 10, 9), 1800, 7);
    assert_eq!(bars.len(), 7);
    assert_eq!(bars[0].date, d(2026, 10, 3));
    assert_eq!(bars[4].sitting_secs, 3600);
    assert_eq!(bars[5].sitting_secs, 7200);
    assert!(bars[6].is_today);
    assert_eq!(bars[6].sitting_secs, 1800);
    assert_eq!(bars[1].sitting_secs, 0);
    assert_eq!(average_secs(&bars), (3600 + 7200 + 1800) / 3);
    assert_eq!(average_secs(&recent_days(&[], d(2026, 10, 9), 0, 7)), 0);
}
