//! Usage history for the dashboard chart: every sample of the last 10
//! minutes, and the peak of each minute for the last 6 hours. Peaks rather
//! than averages, because a commit spike is what makes allocations fail.

use std::collections::VecDeque;

/// Samples kept: 10 minutes at the 2 s sample interval.
const SAMPLES: usize = 300;
/// Minute peaks kept: 6 hours.
const MINUTES: usize = 360;

/// Time span shown by the chart.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Range {
    #[default]
    Minutes10,
    Hour1,
    Hours6,
}

impl Range {
    pub const ALL: [Range; 3] = [Range::Minutes10, Range::Hour1, Range::Hours6];

    pub fn seconds(self) -> f64 {
        match self {
            Range::Minutes10 => 600.0,
            Range::Hour1 => 3_600.0,
            Range::Hours6 => 21_600.0,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Range::Minutes10 => "10m",
            Range::Hour1 => "1h",
            Range::Hours6 => "6h",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.code() == code)
    }
}

/// Points are `[seconds since start, physical %, commit %]`.
#[derive(Default)]
pub struct History {
    samples: VecDeque<[f64; 3]>,
    /// Completed minutes; the time is that of the minute's last sample.
    minutes: VecDeque<[f64; 3]>,
    /// The minute in progress, as `(minute number, peak)`.
    current: Option<(u64, [f64; 3])>,
}

impl History {
    pub fn push(&mut self, t: f64, physical: f64, commit: f64) {
        self.samples.push_back([t, physical, commit]);
        if self.samples.len() > SAMPLES {
            self.samples.pop_front();
        }
        let minute = (t / 60.0) as u64;
        match &mut self.current {
            Some((m, peak)) if *m == minute => *peak = [t, peak[1].max(physical), peak[2].max(commit)],
            current => {
                if let Some((_, done)) = current.replace((minute, [t, physical, commit])) {
                    self.minutes.push_back(done);
                    if self.minutes.len() > MINUTES {
                        self.minutes.pop_front();
                    }
                }
            }
        }
    }

    /// Points within `range` of `now`, oldest first: every sample for the
    /// shortest range, otherwise minute peaks including the minute in progress.
    pub fn points(&self, range: Range, now: f64) -> Vec<[f64; 3]> {
        let recent = |p: &&[f64; 3]| now - p[0] <= range.seconds();
        if range == Range::Minutes10 {
            self.samples.iter().filter(recent).copied().collect()
        } else {
            self.minutes.iter().chain(self.current.as_ref().map(|(_, p)| p)).filter(recent).copied().collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample every 2 s for `seconds`, with physical % = `t` and commit % = 100 - `t` within each minute.
    fn fill(seconds: u32) -> History {
        let mut h = History::default();
        for s in (0..seconds).step_by(2) {
            let within = f64::from(s % 60);
            h.push(f64::from(s), within, 100.0 - within);
        }
        h
    }

    #[test]
    fn short_range_keeps_every_sample_for_ten_minutes() {
        let h = fill(30 * 60);
        let points = h.points(Range::Minutes10, 30.0 * 60.0);
        assert_eq!(points.len(), SAMPLES);
        assert_eq!(points.first().unwrap()[0], 20.0 * 60.0);
    }

    #[test]
    fn longer_ranges_show_the_peak_of_each_minute() {
        let h = fill(3 * 60);
        let points = h.points(Range::Hour1, 3.0 * 60.0);
        // Two completed minutes and the one in progress.
        assert_eq!(points.len(), 3);
        // Peaks come from different samples: physical at the end of the minute, commit at its start.
        assert_eq!(points[0], [58.0, 58.0, 100.0]);
        assert_eq!(points[2], [178.0, 58.0, 100.0]);
    }

    #[test]
    fn minute_peaks_are_capped_at_six_hours() {
        let h = fill(7 * 3600);
        assert_eq!(h.minutes.len(), MINUTES);
        let now = 7.0 * 3600.0;
        assert_eq!(h.points(Range::Hours6, now).len(), MINUTES);
        assert_eq!(h.points(Range::Hour1, now).len(), 60);
    }

    #[test]
    fn range_codes_round_trip() {
        for r in Range::ALL {
            assert_eq!(Range::from_code(r.code()), Some(r));
        }
        assert_eq!(Range::from_code("2h"), None);
    }
}
