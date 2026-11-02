use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced by time arithmetic and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TimeError {
    #[error("denominator must not be zero")]
    ZeroDenominator,
    #[error("invalid frame rate {num}/{den}")]
    InvalidFrameRate { num: u32, den: u32 },
    #[error("invalid frame range {start}..{end}")]
    InvalidRange { start: i64, end: i64 },
    #[error("overflow in time computation")]
    Overflow,
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

fn saturate(v: i128) -> i64 {
    i64::try_from(v).unwrap_or(if v < 0 { i64::MIN } else { i64::MAX })
}

// ---------------------------------------------------------------------------
// FrameRate
// ---------------------------------------------------------------------------

/// Exact frame rate `num / den` frames per second, always reduced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "FrameRateRepr", into = "FrameRateRepr")]
pub struct FrameRate {
    num: u32,
    den: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "FrameRate")]
struct FrameRateRepr {
    num: u32,
    den: u32,
}

impl TryFrom<FrameRateRepr> for FrameRate {
    type Error = TimeError;
    fn try_from(r: FrameRateRepr) -> Result<Self, TimeError> {
        FrameRate::new(r.num, r.den)
    }
}

impl From<FrameRate> for FrameRateRepr {
    fn from(r: FrameRate) -> Self {
        Self { num: r.num, den: r.den }
    }
}

impl FrameRate {
    pub const FPS_23_976: Self = Self { num: 24000, den: 1001 };
    pub const FPS_24: Self = Self { num: 24, den: 1 };
    pub const FPS_25: Self = Self { num: 25, den: 1 };
    pub const FPS_29_97: Self = Self { num: 30000, den: 1001 };
    pub const FPS_30: Self = Self { num: 30, den: 1 };
    pub const FPS_50: Self = Self { num: 50, den: 1 };
    pub const FPS_59_94: Self = Self { num: 60000, den: 1001 };
    pub const FPS_60: Self = Self { num: 60, den: 1 };

    /// Common broadcast/web frame rates with display labels.
    pub const PRESETS: [(&'static str, FrameRate); 8] = [
        ("23.976 fps", Self::FPS_23_976),
        ("24 fps", Self::FPS_24),
        ("25 fps", Self::FPS_25),
        ("29.97 fps", Self::FPS_29_97),
        ("30 fps", Self::FPS_30),
        ("50 fps", Self::FPS_50),
        ("59.94 fps", Self::FPS_59_94),
        ("60 fps", Self::FPS_60),
    ];

    /// Creates a reduced frame rate. Both parts must be non-zero.
    pub fn new(num: u32, den: u32) -> Result<Self, TimeError> {
        if num == 0 || den == 0 {
            return Err(TimeError::InvalidFrameRate { num, den });
        }
        let g = gcd(u128::from(num), u128::from(den));
        let g = u32::try_from(g).map_err(|_| TimeError::Overflow)?;
        Ok(Self {
            num: num / g,
            den: den / g,
        })
    }

    pub const fn num(self) -> u32 {
        self.num
    }

    pub const fn den(self) -> u32 {
        self.den
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }

    /// Integer frames per timecode second (29.97 → 30). Used for non-drop timecode.
    pub fn nominal(self) -> u32 {
        self.num.div_ceil(self.den)
    }

    pub const fn is_integer(self) -> bool {
        self.den == 1
    }

    /// Duration of a single frame.
    pub fn frame_duration(self) -> Time {
        Time {
            num: i64::from(self.den),
            den: i64::from(self.num),
        }
    }
}

impl fmt::Display for FrameRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_integer() {
            write!(f, "{} fps", self.num)
        } else {
            write!(f, "{:.3} fps", self.as_f64())
        }
    }
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

/// Exact time in seconds as a reduced fraction `num / den` with `den > 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "TimeRepr", into = "TimeRepr")]
pub struct Time {
    num: i64,
    den: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "Time")]
struct TimeRepr {
    num: i64,
    den: i64,
}

impl TryFrom<TimeRepr> for Time {
    type Error = TimeError;
    fn try_from(r: TimeRepr) -> Result<Self, TimeError> {
        Time::new(r.num, r.den)
    }
}

impl From<Time> for TimeRepr {
    fn from(t: Time) -> Self {
        Self { num: t.num, den: t.den }
    }
}

impl Time {
    pub const ZERO: Self = Self { num: 0, den: 1 };

    /// Creates a reduced time `num / den` seconds.
    pub fn new(num: i64, den: i64) -> Result<Self, TimeError> {
        Self::from_i128(i128::from(num), i128::from(den))
    }

    fn from_i128(num: i128, den: i128) -> Result<Self, TimeError> {
        if den == 0 {
            return Err(TimeError::ZeroDenominator);
        }
        let (mut n, mut d) = if den < 0 { (-num, -den) } else { (num, den) };
        let g = gcd(n.unsigned_abs(), d.unsigned_abs());
        if g > 1 {
            let g = i128::try_from(g).map_err(|_| TimeError::Overflow)?;
            n /= g;
            d /= g;
        }
        Ok(Self {
            num: i64::try_from(n).map_err(|_| TimeError::Overflow)?,
            den: i64::try_from(d).map_err(|_| TimeError::Overflow)?,
        })
    }

    pub const fn from_secs(secs: i64) -> Self {
        Self { num: secs, den: 1 }
    }

    /// Start time of frame `frame` at `rate`.
    pub fn from_frame(frame: i64, rate: FrameRate) -> Result<Self, TimeError> {
        Self::from_i128(
            i128::from(frame) * i128::from(rate.den),
            i128::from(rate.num),
        )
    }

    /// Converts floating-point seconds to an exact time, snapped to the
    /// nearest frame boundary of `rate`.
    pub fn from_secs_f64(secs: f64, rate: FrameRate) -> Result<Self, TimeError> {
        let frames = (secs * rate.as_f64()).round();
        if !frames.is_finite() || frames.abs() > 9.0e15 {
            return Err(TimeError::Overflow);
        }
        Self::from_frame(frames as i64, rate)
    }

    pub const fn num(self) -> i64 {
        self.num
    }

    pub const fn den(self) -> i64 {
        self.den
    }

    pub fn as_secs_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// `self * rate` as an (unreduced) fraction of frames; denominator > 0.
    fn frames_ratio(self, rate: FrameRate) -> (i128, i128) {
        (
            i128::from(self.num) * i128::from(rate.num),
            i128::from(self.den) * i128::from(rate.den),
        )
    }

    /// Index of the frame that contains this time.
    pub fn to_frame_floor(self, rate: FrameRate) -> i64 {
        let (n, d) = self.frames_ratio(rate);
        saturate(n.div_euclid(d))
    }

    /// Smallest frame index whose start is at or after this time.
    pub fn to_frame_ceil(self, rate: FrameRate) -> i64 {
        let (n, d) = self.frames_ratio(rate);
        saturate(-((-n).div_euclid(d)))
    }

    /// Nearest frame index (ties round up).
    pub fn to_frame_round(self, rate: FrameRate) -> i64 {
        let (n, d) = self.frames_ratio(rate);
        saturate((2 * n + d).div_euclid(2 * d))
    }

    pub fn checked_add(self, other: Self) -> Result<Self, TimeError> {
        let (a, b) = (i128::from(self.num), i128::from(self.den));
        let (c, d) = (i128::from(other.num), i128::from(other.den));
        Self::from_i128(a * d + c * b, b * d)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, TimeError> {
        let (a, b) = (i128::from(self.num), i128::from(self.den));
        let (c, d) = (i128::from(other.num), i128::from(other.den));
        Self::from_i128(a * d - c * b, b * d)
    }

    pub const fn is_negative(self) -> bool {
        self.num < 0
    }
}

impl Ord for Time {
    fn cmp(&self, other: &Self) -> Ordering {
        // Denominators are positive, so cross-multiplication preserves order.
        (i128::from(self.num) * i128::from(other.den))
            .cmp(&(i128::from(other.num) * i128::from(self.den)))
    }
}

impl PartialOrd for Time {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.3} s", self.as_secs_f64())
    }
}

// ---------------------------------------------------------------------------
// Frame ranges and timecode
// ---------------------------------------------------------------------------

/// Half-open, non-empty frame range `start..end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameRange {
    start: i64,
    end: i64,
}

impl FrameRange {
    pub fn new(start: i64, end: i64) -> Result<Self, TimeError> {
        if end <= start {
            return Err(TimeError::InvalidRange { start, end });
        }
        Ok(Self { start, end })
    }

    /// A range covering exactly one frame.
    pub fn single(frame: i64) -> Self {
        Self {
            start: frame,
            end: frame.saturating_add(1).max(frame),
        }
    }

    pub const fn start(self) -> i64 {
        self.start
    }

    /// Exclusive end.
    pub const fn end(self) -> i64 {
        self.end
    }

    /// Last frame inside the range.
    pub fn last(self) -> i64 {
        self.end - 1
    }

    pub fn len(self) -> i64 {
        self.end.saturating_sub(self.start).max(1)
    }

    /// Always `false`: ranges are non-empty by construction.
    pub const fn is_empty(self) -> bool {
        false
    }

    pub fn contains(self, frame: i64) -> bool {
        frame >= self.start && frame < self.end
    }

    pub fn clamp(self, frame: i64) -> i64 {
        frame.clamp(self.start, self.last())
    }
}

/// Formats a frame index as non-drop timecode `HH:MM:SS:FF`.
///
/// For fractional rates (29.97 …) the nominal integer rate is used, as in
/// standard non-drop-frame timecode.
pub fn format_timecode(frame: i64, rate: FrameRate) -> String {
    let fps = u64::from(rate.nominal().max(1));
    let sign = if frame < 0 { "-" } else { "" };
    let f = frame.unsigned_abs();
    let ff = f % fps;
    let total = f / fps;
    let ss = total % 60;
    let mm = (total / 60) % 60;
    let hh = total / 3600;
    format!("{sign}{hh:02}:{mm:02}:{ss:02}:{ff:02}")
}

// ---------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------

/// What happens when playback reaches the end of the range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LoopMode {
    #[default]
    Loop,
    Once,
}

/// Result of [`advance`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackStep {
    pub frame: i64,
    /// `true` when [`LoopMode::Once`] reached the end of the range.
    pub finished: bool,
}

/// Computes the current frame for real-time playback.
///
/// Playback is anchored: `anchor` is the frame at which playback started and
/// `elapsed_secs` the wall-clock time since then. Recomputing from the anchor
/// every frame avoids accumulating drift.
pub fn advance(
    anchor: i64,
    elapsed_secs: f64,
    rate: FrameRate,
    range: FrameRange,
    mode: LoopMode,
) -> PlaybackStep {
    let elapsed = if elapsed_secs.is_finite() && elapsed_secs > 0.0 {
        elapsed_secs
    } else {
        0.0
    };
    // Small epsilon so that e.g. exactly 1.0 s at 30 fps yields 30, not 29.
    let advanced = (elapsed * rate.as_f64() + 1e-9).floor() as i64;
    let offset = (range.clamp(anchor) - range.start()).saturating_add(advanced);
    match mode {
        LoopMode::Loop => PlaybackStep {
            frame: range.start() + offset.rem_euclid(range.len()),
            finished: false,
        },
        LoopMode::Once if offset >= range.len() => PlaybackStep {
            frame: range.last(),
            finished: true,
        },
        LoopMode::Once => PlaybackStep {
            frame: range.start() + offset,
            finished: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rate_is_reduced_and_validated() {
        let r = FrameRate::new(60, 2).unwrap();
        assert_eq!((r.num(), r.den()), (30, 1));
        assert!(FrameRate::new(0, 1).is_err());
        assert!(FrameRate::new(30, 0).is_err());
        assert_eq!(FrameRate::FPS_29_97.nominal(), 30);
        assert_eq!(FrameRate::FPS_23_976.nominal(), 24);
        assert!((FrameRate::FPS_59_94.as_f64() - 59.94).abs() < 0.001);
    }

    #[test]
    fn time_is_normalised() {
        let t = Time::new(10, -4).unwrap();
        assert_eq!((t.num(), t.den()), (-5, 2));
        assert_eq!(Time::new(0, 7).unwrap(), Time::ZERO);
        assert_eq!(Time::new(1, 0), Err(TimeError::ZeroDenominator));
    }

    #[test]
    fn frame_round_trip_is_exact_for_ntsc_rates() {
        for rate in [FrameRate::FPS_29_97, FrameRate::FPS_23_976, FrameRate::FPS_59_94] {
            for frame in [0, 1, 29, 30, 1799, 1800, 107_892, -5] {
                let t = Time::from_frame(frame, rate).unwrap();
                assert_eq!(t.to_frame_floor(rate), frame, "{rate} frame {frame}");
                assert_eq!(t.to_frame_round(rate), frame);
                assert_eq!(t.to_frame_ceil(rate), frame);
            }
        }
    }

    #[test]
    fn frame_rounding_between_frames() {
        let rate = FrameRate::FPS_30;
        // 1/45 s = 0.666 frames at 30 fps
        let t = Time::new(1, 45).unwrap();
        assert_eq!(t.to_frame_floor(rate), 0);
        assert_eq!(t.to_frame_ceil(rate), 1);
        assert_eq!(t.to_frame_round(rate), 1);
        // negative: -1/45 s
        let n = Time::new(-1, 45).unwrap();
        assert_eq!(n.to_frame_floor(rate), -1);
        assert_eq!(n.to_frame_ceil(rate), 0);
    }

    #[test]
    fn ten_seconds_at_29_97_is_300_frames_rounded_up() {
        let d = Time::from_secs(10);
        assert_eq!(d.to_frame_ceil(FrameRate::FPS_29_97), 300);
        assert_eq!(d.to_frame_floor(FrameRate::FPS_29_97), 299);
    }

    #[test]
    fn arithmetic_and_ordering() {
        let a = Time::new(1, 3).unwrap();
        let b = Time::new(1, 6).unwrap();
        assert_eq!(a.checked_add(b).unwrap(), Time::new(1, 2).unwrap());
        assert_eq!(b.checked_sub(a).unwrap(), Time::new(-1, 6).unwrap());
        assert!(a > b);
        assert!(Time::new(-1, 2).unwrap() < Time::ZERO);
        let one = FrameRate::FPS_29_97.frame_duration();
        assert_eq!(one, Time::new(1001, 30000).unwrap());
    }

    #[test]
    fn overflow_is_reported_not_panicking() {
        let big = Time::new(i64::MAX, 1).unwrap();
        assert_eq!(big.checked_add(big), Err(TimeError::Overflow));
        assert_eq!(big.to_frame_floor(FrameRate::FPS_60), i64::MAX);
    }

    #[test]
    fn from_secs_f64_snaps_to_frames() {
        let t = Time::from_secs_f64(1.01, FrameRate::FPS_25).unwrap();
        assert_eq!(t, Time::from_secs(1));
        assert!(Time::from_secs_f64(f64::NAN, FrameRate::FPS_25).is_err());
    }

    #[test]
    fn timecode_formatting() {
        assert_eq!(format_timecode(0, FrameRate::FPS_25), "00:00:00:00");
        assert_eq!(format_timecode(24, FrameRate::FPS_25), "00:00:00:24");
        assert_eq!(format_timecode(25, FrameRate::FPS_25), "00:00:01:00");
        assert_eq!(format_timecode(90_000, FrameRate::FPS_25), "01:00:00:00");
        assert_eq!(format_timecode(1799, FrameRate::FPS_29_97), "00:00:59:29");
        assert_eq!(format_timecode(-30, FrameRate::FPS_30), "-00:00:01:00");
    }

    #[test]
    fn frame_range_basics() {
        assert!(FrameRange::new(5, 5).is_err());
        let r = FrameRange::new(10, 20).unwrap();
        assert_eq!(r.len(), 10);
        assert_eq!(r.last(), 19);
        assert!(r.contains(10) && !r.contains(20));
        assert_eq!(r.clamp(100), 19);
        assert_eq!(r.clamp(-3), 10);
        assert_eq!(FrameRange::single(4).len(), 1);
    }

    #[test]
    fn playback_loops_and_stops() {
        let rate = FrameRate::FPS_30;
        let range = FrameRange::new(0, 60).unwrap();
        let s = advance(0, 1.0, rate, range, LoopMode::Loop);
        assert_eq!(s, PlaybackStep { frame: 30, finished: false });
        let s = advance(50, 1.0, rate, range, LoopMode::Loop);
        assert_eq!(s.frame, 20);
        let s = advance(50, 1.0, rate, range, LoopMode::Once);
        assert_eq!(s, PlaybackStep { frame: 59, finished: true });
        let s = advance(10, 0.5, rate, range, LoopMode::Once);
        assert_eq!(s, PlaybackStep { frame: 25, finished: false });
    }

    #[test]
    fn playback_ignores_invalid_elapsed() {
        let range = FrameRange::new(0, 10).unwrap();
        for e in [f64::NAN, -1.0, f64::NEG_INFINITY] {
            assert_eq!(advance(3, e, FrameRate::FPS_24, range, LoopMode::Loop).frame, 3);
        }
        // Anchor outside the range is clamped first.
        assert_eq!(advance(99, 0.0, FrameRate::FPS_24, range, LoopMode::Loop).frame, 9);
    }

    #[test]
    fn serde_rejects_invalid_values() {
        let ok: FrameRate = ron::from_str("(num: 50, den: 2)").unwrap();
        assert_eq!(ok, FrameRate::FPS_25);
        assert!(ron::from_str::<FrameRate>("(num: 0, den: 1)").is_err());
        assert!(ron::from_str::<Time>("(num: 1, den: 0)").is_err());
    }
}
