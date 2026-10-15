use unseat::{
    average_secs, contrast, palette, recent_days, weekday_initial, CivilDate, DayTotal, Palette,
    Rgba, VisibleState, DARK, LIGHT, WARN_AT,
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
fn overlay_ink_is_legible_on_every_state_surface_and_band() {
    for p in both() {
        let ground = if p.dark { Rgba::opaque(0) } else { Rgba::opaque(0xFFFFFF) };
        for (state, prog) in [
            (VisibleState::Sitting, 0.3),
            (VisibleState::Sitting, 0.9),
            (VisibleState::Overdue, 1.0),
            (VisibleState::Break, 0.5),
            (VisibleState::Away, 0.5),
            (VisibleState::Paused, 0.5),
        ] {
            let c = p.overlay(state, prog);
            for surface in [c.bg.over(ground), c.band.over(ground)] {
                // Digits are large text (3:1); the small tag needs 4.5:1.
                assert!(contrast(c.digits, surface) >= 4.5, "digits dark={} {state:?}", p.dark);
                assert!(contrast(c.tag, surface) >= 4.5, "tag dark={} {state:?}", p.dark);
            }
        }
        assert!(contrast(p.on_alarm, p.alarm) >= 4.5, "alarm red dark={}", p.dark);
        assert!(contrast(p.on_selected, p.selected) >= 4.5, "selected chip dark={}", p.dark);
    }
}

#[test]
fn overlay_state_colors_follow_the_state() {
    let p = palette(true);
    assert_eq!(p.overlay(VisibleState::Sitting, 0.2), p.sit);
    assert_eq!(p.overlay(VisibleState::Sitting, WARN_AT), p.warn);
    assert_eq!(p.overlay(VisibleState::Overdue, 1.0), p.over);
    assert_eq!(p.overlay(VisibleState::Break, 0.5), p.brk);
    assert_eq!(p.overlay(VisibleState::Away, 0.5), p.idle);
    assert_eq!(p.overlay(VisibleState::Paused, 0.5), p.idle);
    assert_ne!(p.sit.bg, p.over.bg);
    assert!(!palette(false).dark);
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

#[test]
fn prompt_tabs_and_caption_are_legible_and_quiet() {
    for p in both() {
        let c = p.prompt;
        for bg in [c.primary, c.primary_hover] {
            assert!(contrast(c.on_primary, bg) >= 4.5, "primary tab dark={}", p.dark);
        }
        for bg in [c.secondary, c.secondary_hover] {
            assert!(contrast(c.on_secondary, bg) >= 4.5, "secondary tab dark={}", p.dark);
        }
        assert_eq!(c.caption_bg.3, 255, "the caption chip is opaque, so any desktop behind it is fine");
        assert!(contrast(c.caption, c.caption_bg) >= 4.5, "caption dark={}", p.dark);
        // The primary tab reads as the overdue pill's shadow side, never brighter than it.
        let black = Rgba::opaque(0);
        assert!(contrast(c.primary, black) < contrast(p.over.bg, black), "primary darker than the pill");
    }
}
