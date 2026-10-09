use crate::date::CivilDate;
use crate::settings::DayTotal;

/// One bar of the weekly chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayBar {
    pub date: CivilDate,
    pub sitting_secs: u64,
    pub is_today: bool,
}

/// The last `days` days ending today, oldest first. Days without a record are zero.
pub fn recent_days(history: &[DayTotal], today: CivilDate, today_secs: u64, days: usize) -> Vec<DayBar> {
    (0..days as i64)
        .rev()
        .map(|back| {
            let date = today.add_days(-back);
            let sitting_secs = if back == 0 {
                today_secs
            } else {
                let key = date.to_ymd_string();
                history
                    .iter()
                    .find(|d| d.date == key)
                    .map_or(0, |d| d.sitting_secs)
            };
            DayBar {
                date,
                sitting_secs,
                is_today: back == 0,
            }
        })
        .collect()
}

/// Average over the days that have any sitting time, so a fresh install is not dragged down
/// by empty days.
pub fn average_secs(bars: &[DayBar]) -> u64 {
    let used: Vec<u64> = bars.iter().map(|b| b.sitting_secs).filter(|&s| s > 0).collect();
    if used.is_empty() {
        0
    } else {
        used.iter().sum::<u64>() / used.len() as u64
    }
}

pub fn weekday_initial(date: CivilDate) -> &'static str {
    ["M", "T", "W", "T", "F", "S", "S"][usize::from(date.weekday())]
}
