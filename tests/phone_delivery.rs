use std::time::Duration;
use unseat::companion::outbox::{Note, Outbox, BACKOFF_MAX_MS, MAX_JOBS_PER_DEVICE};
use unseat::companion::reminder::{message, phone_should_alert, reminder_for, Presence, ReminderKind};
use unseat::companion::webpush::Outcome;
use unseat::{Beep, PhoneAlerts, Settings};

fn on() -> Settings {
    Settings { phone_enabled: true, ..Settings::default() }
}

const AT_DESK: Presence = Presence { idle_for: Duration::from_secs(2), locked: false };

// ---------------------------------------------------------------- which reminders

#[test]
fn every_engine_reminder_is_mirrored_including_sound_only_repeats() {
    let s = on();
    let sitting = Duration::from_secs(3600 + 125);
    let time_up = reminder_for(Beep::LimitReached, &s, sitting).unwrap();
    let repeat = reminder_for(Beep::Progressive, &s, sitting).unwrap();
    let over = reminder_for(Beep::BreakOver, &s, sitting).unwrap();
    assert_eq!(time_up.kind, ReminderKind::TimeUp);
    assert_eq!(repeat.kind, ReminderKind::Repeat);
    assert_eq!(over.kind, ReminderKind::BreakOver);
    assert!(reminder_for(Beep::None, &s, sitting).is_none());
    assert!(time_up.body.starts_with("You've been sitting for "), "{}", time_up.body);
    assert_eq!(time_up.title, "Time to stand up");
    assert_eq!(repeat.title, "Still sitting");
    assert_eq!(over.title, "Break's over");
}

#[test]
fn windows_sound_and_toast_switches_do_not_silence_the_phone() {
    let s = Settings { sound_enabled: false, notify_enabled: false, alert_volume: 0, ..on() };
    assert!(reminder_for(Beep::Progressive, &s, Duration::from_secs(4000)).is_some());
    assert!(phone_should_alert(&s, AT_DESK));
}

#[test]
fn the_phone_switch_is_off_by_default() {
    assert!(!phone_should_alert(&Settings::default(), AT_DESK));
}

#[test]
fn idle_only_waits_until_the_user_is_away() {
    let s = Settings { phone_alerts: PhoneAlerts::IdleOnly, idle_after_secs: 60, ..on() };
    assert!(!phone_should_alert(&s, AT_DESK));
    assert!(!phone_should_alert(&s, Presence { idle_for: Duration::from_secs(59), locked: false }));
    assert!(phone_should_alert(&s, Presence { idle_for: Duration::from_secs(60), locked: false }));
    assert!(phone_should_alert(&s, Presence { idle_for: Duration::ZERO, locked: true }));
}

#[test]
fn sitting_reminders_expire_at_the_next_repeat_and_never_live_long() {
    let s = Settings { repeat_every_secs: 300, ..on() };
    assert_eq!(reminder_for(Beep::LimitReached, &s, Duration::ZERO).unwrap().ttl_secs, 300);
    let s = Settings { repeat_every_secs: 3600, ..on() };
    assert_eq!(reminder_for(Beep::LimitReached, &s, Duration::ZERO).unwrap().ttl_secs, 900);
    let s = Settings { repeat_reminders: false, ..on() };
    assert_eq!(reminder_for(Beep::LimitReached, &s, Duration::ZERO).unwrap().ttl_secs, 900);
    assert_eq!(reminder_for(Beep::BreakOver, &s, Duration::ZERO).unwrap().ttl_secs, 600);
}

#[test]
fn the_push_is_a_declarative_message_tagged_by_topic() {
    let r = reminder_for(Beep::LimitReached, &on(), Duration::from_secs(3600)).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&message(&r, "https://pc.example.ts.net/")).unwrap();
    assert_eq!(v["web_push"], 8030);
    assert_eq!(v["notification"]["title"], "Time to stand up");
    assert_eq!(v["notification"]["navigate"], "https://pc.example.ts.net/");
    assert_eq!(v["notification"]["tag"], "unseat-sitting");
    assert_eq!(ReminderKind::Repeat.topic(), ReminderKind::TimeUp.topic());
    assert_ne!(ReminderKind::BreakOver.topic(), ReminderKind::TimeUp.topic());
}

// ---------------------------------------------------------------- delivery queue

fn devs(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

#[test]
fn one_job_per_device_and_each_is_handed_out_once() {
    let mut o = Outbox::new();
    o.enqueue(ReminderKind::TimeUp, b"x".to_vec(), 300, &devs(&["a", "b"]), 1_000);
    let (a, _) = o.take_due(1_000);
    let (b, _) = o.take_due(1_000);
    let (none, _) = o.take_due(1_000);
    assert_eq!(a.as_ref().unwrap().device, "a");
    assert_eq!(b.as_ref().unwrap().device, "b");
    assert!(none.is_none(), "in-flight jobs are not handed out twice");
    let notes = o.finish(a.unwrap().id, Outcome::Accepted, Some(201), 1_500);
    assert!(matches!(&notes[0], Note::Delivered { device, after_ms: 500, .. } if device == "a"));
}

#[test]
fn finishing_twice_does_nothing_the_second_time() {
    let mut o = Outbox::new();
    o.enqueue(ReminderKind::TimeUp, b"x".to_vec(), 300, &devs(&["a"]), 0);
    let job = o.take_due(0).0.unwrap();
    assert_eq!(o.finish(job.id, Outcome::Accepted, Some(201), 10).len(), 1);
    assert!(o.finish(job.id, Outcome::Accepted, Some(201), 20).is_empty());
    assert!(o.finish(job.id, Outcome::Retry, None, 30).is_empty());
    assert_eq!(o.pending(), 0);
}

#[test]
fn a_newer_sitting_reminder_replaces_a_queued_one_but_not_a_break_reminder() {
    let mut o = Outbox::new();
    o.enqueue(ReminderKind::BreakOver, b"b".to_vec(), 600, &devs(&["a"]), 0);
    o.enqueue(ReminderKind::TimeUp, b"1".to_vec(), 300, &devs(&["a"]), 0);
    let (_, notes) = o.enqueue(ReminderKind::Repeat, b"2".to_vec(), 300, &devs(&["a"]), 100);
    assert!(matches!(&notes[..], [Note::Replaced { kind: ReminderKind::TimeUp, .. }]));
    assert_eq!(o.pending(), 2);
    let mut kinds = vec![];
    while let (Some(j), _) = o.take_due(200) {
        kinds.push((j.kind, j.payload));
    }
    assert!(kinds.contains(&(ReminderKind::BreakOver, b"b".to_vec())));
    assert!(kinds.contains(&(ReminderKind::Repeat, b"2".to_vec())));
}

#[test]
fn a_replaced_job_that_was_in_flight_is_not_retried() {
    let mut o = Outbox::new();
    o.enqueue(ReminderKind::TimeUp, b"1".to_vec(), 300, &devs(&["a"]), 0);
    let old = o.take_due(0).0.unwrap();
    o.enqueue(ReminderKind::Repeat, b"2".to_vec(), 300, &devs(&["a"]), 10);
    assert!(o.finish(old.id, Outcome::Retry, None, 20).is_empty());
    let next = o.take_due(1_000_000).0;
    assert!(next.is_none() || next.unwrap().payload == b"2".to_vec());
}

#[test]
fn network_failures_back_off_and_then_count_as_missed_at_expiry() {
    let mut o = Outbox::new();
    o.enqueue(ReminderKind::TimeUp, b"x".to_vec(), 60, &devs(&["a"]), 0);
    let mut now = 0;
    let mut waits = vec![];
    loop {
        let (job, notes) = o.take_due(now);
        if let Some(Note::Missed { .. }) = notes.first() {
            break;
        }
        let Some(job) = job else {
            now = o.next_wake_ms().expect("still queued");
            continue;
        };
        match o.finish(job.id, Outcome::Retry, None, now).pop() {
            Some(Note::Retrying { at_ms, .. }) => {
                waits.push(at_ms - now);
                now = at_ms;
            }
            Some(Note::Missed { .. }) => break,
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(&waits[..4], &[2_000, 4_000, 8_000, 16_000]);
    assert!(waits.iter().all(|w| *w <= BACKOFF_MAX_MS));
    assert_eq!(o.pending(), 0, "an expired reminder is never sent late");
}

#[test]
fn reminders_that_expire_while_queued_are_reported_missed_not_sent() {
    let mut o = Outbox::new();
    o.enqueue(ReminderKind::BreakOver, b"x".to_vec(), 60, &devs(&["a"]), 0);
    let (job, notes) = o.take_due(60_000);
    assert!(job.is_none());
    assert!(matches!(&notes[..], [Note::Missed { kind: ReminderKind::BreakOver, .. }]));
}

#[test]
fn ttl_sent_to_apple_is_what_is_left_of_the_reminders_life() {
    let mut o = Outbox::new();
    o.enqueue(ReminderKind::TimeUp, b"x".to_vec(), 300, &devs(&["a"]), 0);
    let job = o.take_due(0).0.unwrap();
    o.finish(job.id, Outcome::Retry, None, 0);
    let again = o.take_due(2_000).0.unwrap();
    assert_eq!(again.remaining_ttl_secs(2_000), 298);
    assert_eq!(again.attempts, 2);
}

#[test]
fn a_gone_subscription_drops_that_devices_jobs_only() {
    let mut o = Outbox::new();
    o.enqueue(ReminderKind::TimeUp, b"x".to_vec(), 300, &devs(&["a", "b"]), 0);
    o.enqueue(ReminderKind::BreakOver, b"y".to_vec(), 300, &devs(&["a"]), 0);
    let job = loop {
        let j = o.take_due(0).0.unwrap();
        if j.device == "a" {
            break j;
        }
        o.finish(j.id, Outcome::Accepted, Some(201), 0);
    };
    let notes = o.finish(job.id, Outcome::Gone, Some(410), 1);
    assert_eq!(notes, vec![Note::Gone { device: "a".into() }]);
    assert_eq!(o.pending_for("a"), 0);
}

#[test]
fn rejected_requests_are_not_retried() {
    let mut o = Outbox::new();
    o.enqueue(ReminderKind::TimeUp, b"x".to_vec(), 300, &devs(&["a"]), 0);
    let job = o.take_due(0).0.unwrap();
    let notes = o.finish(job.id, Outcome::Rejected, Some(400), 1);
    assert!(matches!(&notes[..], [Note::Rejected { status: Some(400), .. }]));
    assert_eq!(o.pending(), 0);
}

#[test]
fn acting_on_the_pc_cancels_queued_sitting_reminders() {
    let mut o = Outbox::new();
    o.enqueue(ReminderKind::TimeUp, b"x".to_vec(), 300, &devs(&["a"]), 0);
    o.enqueue(ReminderKind::BreakOver, b"y".to_vec(), 300, &devs(&["a"]), 0);
    let notes = o.cancel_topic(ReminderKind::TimeUp.topic());
    assert!(matches!(&notes[..], [Note::Cancelled { kind: ReminderKind::TimeUp, .. }]));
    assert_eq!(o.take_due(0).0.unwrap().kind, ReminderKind::BreakOver);
}

#[test]
fn a_removed_device_gets_nothing_more() {
    let mut o = Outbox::new();
    o.enqueue(ReminderKind::TimeUp, b"x".to_vec(), 300, &devs(&["a", "b"]), 0);
    o.drop_device("a");
    let job = o.take_due(0).0.unwrap();
    assert_eq!(job.device, "b");
    assert!(o.take_due(0).0.is_none());
}

#[test]
fn a_phone_that_stays_unreachable_never_builds_a_backlog() {
    let mut o = Outbox::new();
    let kinds = [ReminderKind::TimeUp, ReminderKind::Repeat, ReminderKind::BreakOver, ReminderKind::Test];
    for i in 0..100u64 {
        o.enqueue(kinds[(i % 4) as usize], vec![i as u8], 900, &devs(&["a"]), i);
    }
    // At most one queued reminder per topic (sitting, break, test).
    assert_eq!(o.pending_for("a"), 3);
    assert!(o.pending_for("a") <= MAX_JOBS_PER_DEVICE);
}

#[test]
fn the_sender_wakes_for_the_earliest_retry() {
    let mut o = Outbox::new();
    assert_eq!(o.next_wake_ms(), None);
    o.enqueue(ReminderKind::TimeUp, b"x".to_vec(), 300, &devs(&["a"]), 5);
    assert_eq!(o.next_wake_ms(), Some(5));
    let job = o.take_due(5).0.unwrap();
    assert_eq!(o.next_wake_ms(), None, "in-flight job: nothing to wake for");
    o.finish(job.id, Outcome::Retry, None, 5);
    assert_eq!(o.next_wake_ms(), Some(2_005));
}
