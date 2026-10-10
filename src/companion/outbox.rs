//! Delivery queue for phone reminders. Pure state machine: the host asks for due jobs, sends
//! them, and reports the outcome. Times are PC wall-clock milliseconds; the phone's clock is
//! never used (Phase 1 found one 41 s off).
//!
//! Rules:
//! - Each reminder becomes one job per subscribed device, with an expiry (its TTL).
//! - A newer reminder on the same topic replaces a queued older one (no backlog of stale
//!   "stand up" pushes). This only covers the PC's queue: Apple did not collapse held pushes
//!   by `Topic` in live testing, and may deliver one shortly after its TTL.
//! - Network errors, 429 and 5xx retry with backoff (2 s doubling to 60 s) until expiry; then
//!   the reminder counts as missed.
//! - 404/410 means the subscription is gone: the device's jobs are dropped and the host marks
//!   it for re-subscription.
//! - A job is handed out once at a time and finishes once, so nothing is sent twice.

use super::reminder::ReminderKind;
use super::webpush::Outcome;

pub const BACKOFF_START_MS: u64 = 2_000;
pub const BACKOFF_MAX_MS: u64 = 60_000;
/// Queued jobs per device; older ones are dropped first if a phone stays unreachable.
pub const MAX_JOBS_PER_DEVICE: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub event: u64,
    pub device: String,
    pub kind: ReminderKind,
    pub payload: Vec<u8>,
    pub ttl_secs: u64,
    pub created_ms: u64,
    pub expires_ms: u64,
    pub attempts: u32,
    pub next_try_ms: u64,
    in_flight: bool,
    /// Replaced or cancelled while it was being sent: don't retry it.
    dead: bool,
}

impl Job {
    /// TTL for the push service: what is left of the reminder's useful life.
    pub fn remaining_ttl_secs(&self, now_ms: u64) -> u64 {
        self.expires_ms.saturating_sub(now_ms) / 1000
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    Delivered { device: String, kind: ReminderKind, event: u64, attempts: u32, after_ms: u64 },
    Retrying { device: String, kind: ReminderKind, event: u64, attempt: u32, at_ms: u64 },
    Missed { device: String, kind: ReminderKind, event: u64 },
    Replaced { device: String, kind: ReminderKind, event: u64 },
    Cancelled { device: String, kind: ReminderKind, event: u64 },
    Gone { device: String },
    Rejected { device: String, kind: ReminderKind, event: u64, status: Option<u16> },
}

#[derive(Default)]
pub struct Outbox {
    jobs: Vec<Job>,
    next_id: u64,
    next_event: u64,
}

impl Outbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// The event number the next `enqueue` will use.
    pub fn next_event(&self) -> u64 {
        self.next_event + 1
    }

    pub fn pending(&self) -> usize {
        self.jobs.iter().filter(|j| !j.dead).count()
    }

    pub fn pending_for(&self, device: &str) -> usize {
        self.jobs.iter().filter(|j| !j.dead && j.device == device).count()
    }

    /// Queue one reminder for each device. Returns the event number and what was replaced.
    pub fn enqueue(&mut self, kind: ReminderKind, payload: Vec<u8>, ttl_secs: u64, devices: &[String], now_ms: u64) -> (u64, Vec<Note>) {
        self.next_event += 1;
        let event = self.next_event;
        let mut notes = Vec::new();
        for device in devices {
            for j in self.jobs.iter_mut().filter(|j| !j.dead && &j.device == device && j.kind.topic() == kind.topic()) {
                j.dead = true;
                notes.push(Note::Replaced { device: j.device.clone(), kind: j.kind, event: j.event });
            }
            self.next_id += 1;
            self.jobs.push(Job {
                id: self.next_id,
                event,
                device: device.clone(),
                kind,
                payload: payload.clone(),
                ttl_secs,
                created_ms: now_ms,
                expires_ms: now_ms + ttl_secs * 1000,
                attempts: 0,
                next_try_ms: now_ms,
                in_flight: false,
                dead: false,
            });
            // Keep the queue bounded: drop the oldest live jobs for this device.
            let mut live: Vec<u64> = self.jobs.iter().filter(|j| !j.dead && &j.device == device && !j.in_flight).map(|j| j.id).collect();
            while live.len() > MAX_JOBS_PER_DEVICE {
                let oldest = live.remove(0);
                if let Some(j) = self.jobs.iter_mut().find(|j| j.id == oldest) {
                    j.dead = true;
                    notes.push(Note::Missed { device: j.device.clone(), kind: j.kind, event: j.event });
                }
            }
        }
        self.sweep();
        (event, notes)
    }

    /// The user acted on the PC (reset, break, snooze, pause): queued reminders on this topic
    /// are obsolete.
    pub fn cancel_topic(&mut self, topic: &str) -> Vec<Note> {
        let mut notes = Vec::new();
        for j in self.jobs.iter_mut().filter(|j| !j.dead && j.kind.topic() == topic) {
            j.dead = true;
            notes.push(Note::Cancelled { device: j.device.clone(), kind: j.kind, event: j.event });
        }
        self.sweep();
        notes
    }

    /// A device was revoked or unsubscribed.
    pub fn drop_device(&mut self, device: &str) {
        for j in self.jobs.iter_mut().filter(|j| j.device == device) {
            j.dead = true;
        }
        self.sweep();
    }

    /// The next job to send, marked in flight. Expired jobs are removed first and reported.
    pub fn take_due(&mut self, now_ms: u64) -> (Option<Job>, Vec<Note>) {
        let mut notes = Vec::new();
        for j in self.jobs.iter_mut().filter(|j| !j.dead && !j.in_flight && now_ms >= j.expires_ms) {
            j.dead = true;
            notes.push(Note::Missed { device: j.device.clone(), kind: j.kind, event: j.event });
        }
        self.sweep();
        let due = self
            .jobs
            .iter_mut()
            .filter(|j| !j.dead && !j.in_flight && j.next_try_ms <= now_ms)
            .min_by_key(|j| (j.next_try_ms, j.id));
        let job = due.map(|j| {
            j.in_flight = true;
            j.attempts += 1;
            j.clone()
        });
        (job, notes)
    }

    /// When the sender should next look: the earliest retry or expiry of a waiting job.
    pub fn next_wake_ms(&self) -> Option<u64> {
        self.jobs.iter().filter(|j| !j.dead && !j.in_flight).map(|j| j.next_try_ms.min(j.expires_ms)).min()
    }

    /// Report how a send went. Unknown or already finished jobs are ignored.
    pub fn finish(&mut self, job_id: u64, outcome: Outcome, status: Option<u16>, now_ms: u64) -> Vec<Note> {
        let Some(pos) = self.jobs.iter().position(|j| j.id == job_id && j.in_flight) else {
            return Vec::new();
        };
        let mut notes = Vec::new();
        let job = &mut self.jobs[pos];
        job.in_flight = false;
        match outcome {
            Outcome::Accepted => {
                notes.push(Note::Delivered {
                    device: job.device.clone(),
                    kind: job.kind,
                    event: job.event,
                    attempts: job.attempts,
                    after_ms: now_ms.saturating_sub(job.created_ms),
                });
                job.dead = true;
            }
            Outcome::Retry if job.dead => {}
            Outcome::Retry => {
                let shift = job.attempts.saturating_sub(1).min(16);
                let wait = (BACKOFF_START_MS << shift).min(BACKOFF_MAX_MS);
                let at = now_ms + wait;
                if at >= job.expires_ms {
                    job.dead = true;
                    notes.push(Note::Missed { device: job.device.clone(), kind: job.kind, event: job.event });
                } else {
                    job.next_try_ms = at;
                    notes.push(Note::Retrying { device: job.device.clone(), kind: job.kind, event: job.event, attempt: job.attempts, at_ms: at });
                }
            }
            Outcome::Gone => {
                let device = job.device.clone();
                for j in self.jobs.iter_mut().filter(|j| j.device == device) {
                    j.dead = true;
                }
                notes.push(Note::Gone { device });
            }
            Outcome::Rejected => {
                job.dead = true;
                notes.push(Note::Rejected { device: job.device.clone(), kind: job.kind, event: job.event, status });
            }
        }
        self.sweep();
        notes
    }

    fn sweep(&mut self) {
        self.jobs.retain(|j| !j.dead || j.in_flight);
    }
}
