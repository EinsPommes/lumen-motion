use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::time::Time;

/// An animatable scalar property of a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Channel {
    PositionX,
    PositionY,
    ScaleX,
    ScaleY,
    Rotation,
    Opacity,
}

impl Channel {
    pub const ALL: [Channel; 6] = [
        Channel::PositionX,
        Channel::PositionY,
        Channel::ScaleX,
        Channel::ScaleY,
        Channel::Rotation,
        Channel::Opacity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Channel::PositionX => "Position X",
            Channel::PositionY => "Position Y",
            Channel::ScaleX => "Scale X",
            Channel::ScaleY => "Scale Y",
            Channel::Rotation => "Rotation",
            Channel::Opacity => "Opacity",
        }
    }
}

/// How a curve moves from one keyframe to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interpolation {
    Linear,
    #[default]
    Bezier,
    Hold,
}

impl Interpolation {
    pub const ALL: [Interpolation; 3] = [
        Interpolation::Linear,
        Interpolation::Bezier,
        Interpolation::Hold,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Interpolation::Linear => "Linear",
            Interpolation::Bezier => "Bezier",
            Interpolation::Hold => "Hold",
        }
    }
}

/// Bezier handle relative to its keyframe.
///
/// `dt` is the handle length as a fraction of the segment duration (0..=1),
/// `dv` the value offset. The default gives a flat ease in/out.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tangent {
    pub dt: f32,
    pub dv: f32,
}

impl Tangent {
    pub const EASE: Tangent = Tangent {
        dt: 1.0 / 3.0,
        dv: 0.0,
    };
}

impl Default for Tangent {
    fn default() -> Self {
        Self::EASE
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub time: Time,
    pub value: f32,
    /// Interpolation of the segment that starts at this keyframe.
    #[serde(default)]
    pub interpolation: Interpolation,
    #[serde(default)]
    pub in_tangent: Tangent,
    #[serde(default)]
    pub out_tangent: Tangent,
}

impl Keyframe {
    pub fn new(time: Time, value: f32) -> Self {
        Self {
            time,
            value,
            interpolation: Interpolation::default(),
            in_tangent: Tangent::default(),
            out_tangent: Tangent::default(),
        }
    }
}

/// Keyframes of one channel, sorted by time with unique times.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "Vec<Keyframe>", into = "Vec<Keyframe>")]
pub struct Curve {
    keys: Vec<Keyframe>,
}

impl From<Vec<Keyframe>> for Curve {
    fn from(mut keys: Vec<Keyframe>) -> Self {
        keys.sort_by_key(|k| k.time);
        // Keep the last of several keys at the same time.
        keys.reverse();
        keys.dedup_by(|a, b| a.time == b.time);
        keys.reverse();
        Self { keys }
    }
}

impl From<Curve> for Vec<Keyframe> {
    fn from(c: Curve) -> Self {
        c.keys
    }
}

impl Curve {
    pub fn keys(&self) -> &[Keyframe] {
        &self.keys
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    fn find(&self, time: Time) -> Result<usize, usize> {
        self.keys.binary_search_by(|k| k.time.cmp(&time))
    }

    pub fn key_at(&self, time: Time) -> Option<&Keyframe> {
        self.find(time).ok().and_then(|i| self.keys.get(i))
    }

    pub fn key_at_mut(&mut self, time: Time) -> Option<&mut Keyframe> {
        let i = self.find(time).ok()?;
        self.keys.get_mut(i)
    }

    /// Sets the value at `time`, creating a keyframe if none exists there.
    pub fn set(&mut self, time: Time, value: f32) {
        match self.find(time) {
            Ok(i) => {
                if let Some(k) = self.keys.get_mut(i) {
                    k.value = value;
                }
            }
            Err(i) => self.keys.insert(i, Keyframe::new(time, value)),
        }
    }

    /// Inserts `key`, replacing any keyframe at the same time.
    pub fn insert(&mut self, key: Keyframe) -> Option<Keyframe> {
        match self.find(key.time) {
            Ok(i) => self.keys.get_mut(i).map(|k| std::mem::replace(k, key)),
            Err(i) => {
                self.keys.insert(i, key);
                None
            }
        }
    }

    pub fn remove(&mut self, time: Time) -> Option<Keyframe> {
        self.find(time).ok().map(|i| self.keys.remove(i))
    }

    /// Moves the keyframe at `from` to `to`, replacing a keyframe at `to`.
    pub fn move_key(&mut self, from: Time, to: Time) -> bool {
        if from == to {
            return self.key_at(from).is_some();
        }
        let Some(mut key) = self.remove(from) else {
            return false;
        };
        key.time = to;
        self.insert(key);
        true
    }

    /// Neighbouring keyframes around `time`: (last at or before, first after).
    pub fn segment(&self, time: Time) -> (Option<&Keyframe>, Option<&Keyframe>) {
        match self.find(time) {
            Ok(i) => (self.keys.get(i), self.keys.get(i + 1)),
            Err(i) => (
                i.checked_sub(1).and_then(|j| self.keys.get(j)),
                self.keys.get(i),
            ),
        }
    }
}

/// Keyframed channels of a layer. Channels without keyframes are absent and
/// use the layer's static value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Animation {
    channels: BTreeMap<Channel, Curve>,
}

impl Animation {
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    /// The curve of `channel`, if it has keyframes.
    pub fn curve(&self, channel: Channel) -> Option<&Curve> {
        self.channels.get(&channel).filter(|c| !c.is_empty())
    }

    pub fn is_animated(&self, channel: Channel) -> bool {
        self.curve(channel).is_some()
    }

    pub fn animated_channels(&self) -> impl Iterator<Item = Channel> + '_ {
        self.channels
            .iter()
            .filter(|(_, c)| !c.is_empty())
            .map(|(ch, _)| *ch)
    }

    pub fn has_key(&self, channel: Channel, time: Time) -> bool {
        self.curve(channel).is_some_and(|c| c.key_at(time).is_some())
    }

    pub fn set_key(&mut self, channel: Channel, time: Time, value: f32) {
        self.channels.entry(channel).or_default().set(time, value);
    }

    pub fn remove_key(&mut self, channel: Channel, time: Time) {
        if let Some(curve) = self.channels.get_mut(&channel) {
            curve.remove(time);
            if curve.is_empty() {
                self.channels.remove(&channel);
            }
        }
    }

    pub fn clear_channel(&mut self, channel: Channel) {
        self.channels.remove(&channel);
    }

    /// Sorted, unique keyframe times across `channels`.
    pub fn key_times(&self, channels: &[Channel]) -> Vec<Time> {
        let mut times: Vec<Time> = channels
            .iter()
            .filter_map(|ch| self.curve(*ch))
            .flat_map(|c| c.keys().iter().map(|k| k.time))
            .collect();
        times.sort();
        times.dedup();
        times
    }

    pub fn move_keys(&mut self, channels: &[Channel], from: Time, to: Time) {
        for ch in channels {
            if let Some(curve) = self.channels.get_mut(ch) {
                curve.move_key(from, to);
            }
        }
    }

    pub fn remove_keys(&mut self, channels: &[Channel], time: Time) {
        for ch in channels {
            self.remove_key(*ch, time);
        }
    }

    pub fn set_interpolation(&mut self, channels: &[Channel], time: Time, interp: Interpolation) {
        for ch in channels {
            if let Some(key) = self.channels.get_mut(ch).and_then(|c| c.key_at_mut(time)) {
                key.interpolation = interp;
            }
        }
    }

    /// Interpolation shared by all keys at `time` in `channels`, if uniform.
    pub fn interpolation_at(&self, channels: &[Channel], time: Time) -> Option<Interpolation> {
        let mut found = None;
        for key in channels
            .iter()
            .filter_map(|ch| self.curve(*ch).and_then(|c| c.key_at(time)))
        {
            match found {
                None => found = Some(key.interpolation),
                Some(i) if i != key.interpolation => return None,
                Some(_) => {}
            }
        }
        found
    }

    /// Last key time strictly before `time` across `channels`.
    pub fn previous_key(&self, channels: &[Channel], time: Time) -> Option<Time> {
        self.key_times(channels).into_iter().rev().find(|t| *t < time)
    }

    /// First key time strictly after `time` across `channels`.
    pub fn next_key(&self, channels: &[Channel], time: Time) -> Option<Time> {
        self.key_times(channels).into_iter().find(|t| *t > time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: i64) -> Time {
        Time::from_secs(n)
    }

    #[test]
    fn set_inserts_sorted_and_replaces() {
        let mut c = Curve::default();
        c.set(t(2), 2.0);
        c.set(t(0), 0.0);
        c.set(t(1), 1.0);
        c.set(t(1), 5.0);
        let times: Vec<_> = c.keys().iter().map(|k| k.time).collect();
        assert_eq!(times, vec![t(0), t(1), t(2)]);
        assert_eq!(c.key_at(t(1)).unwrap().value, 5.0);
    }

    #[test]
    fn move_key_overwrites_target() {
        let mut c = Curve::default();
        c.set(t(0), 0.0);
        c.set(t(1), 1.0);
        assert!(c.move_key(t(0), t(1)));
        assert_eq!(c.len(), 1);
        assert_eq!(c.key_at(t(1)).unwrap().value, 0.0);
        assert!(!c.move_key(t(5), t(6)));
    }

    #[test]
    fn segment_lookup() {
        let mut c = Curve::default();
        c.set(t(0), 0.0);
        c.set(t(2), 2.0);
        let (a, b) = c.segment(t(1));
        assert_eq!((a.unwrap().time, b.unwrap().time), (t(0), t(2)));
        let (a, b) = c.segment(t(2));
        assert_eq!(a.unwrap().time, t(2));
        assert!(b.is_none());
        let (a, b) = c.segment(Time::new(-1, 2).unwrap());
        assert!(a.is_none());
        assert_eq!(b.unwrap().time, t(0));
    }

    #[test]
    fn deserialisation_sorts_and_dedups() {
        let keys = vec![Keyframe::new(t(3), 3.0), Keyframe::new(t(1), 1.0), Keyframe::new(t(3), 9.0)];
        let c = Curve::from(keys);
        assert_eq!(c.len(), 2);
        assert_eq!(c.key_at(t(3)).unwrap().value, 9.0);
    }

    #[test]
    fn animation_removes_empty_channels() {
        let mut a = Animation::default();
        a.set_key(Channel::Rotation, t(0), 10.0);
        assert!(a.is_animated(Channel::Rotation));
        a.remove_key(Channel::Rotation, t(0));
        assert!(!a.is_animated(Channel::Rotation));
        assert!(a.is_empty());
    }

    #[test]
    fn key_times_and_navigation() {
        let mut a = Animation::default();
        a.set_key(Channel::PositionX, t(0), 0.0);
        a.set_key(Channel::PositionY, t(2), 0.0);
        a.set_key(Channel::Opacity, t(4), 1.0);
        let pos = [Channel::PositionX, Channel::PositionY];
        assert_eq!(a.key_times(&pos), vec![t(0), t(2)]);
        assert_eq!(a.next_key(&Channel::ALL, t(2)), Some(t(4)));
        assert_eq!(a.previous_key(&Channel::ALL, t(2)), Some(t(0)));
        assert_eq!(a.previous_key(&Channel::ALL, t(0)), None);
    }

    #[test]
    fn interpolation_is_reported_when_uniform() {
        let mut a = Animation::default();
        a.set_key(Channel::ScaleX, t(1), 1.0);
        a.set_key(Channel::ScaleY, t(1), 1.0);
        let ch = [Channel::ScaleX, Channel::ScaleY];
        assert_eq!(a.interpolation_at(&ch, t(1)), Some(Interpolation::Bezier));
        a.set_interpolation(&[Channel::ScaleX], t(1), Interpolation::Hold);
        assert_eq!(a.interpolation_at(&ch, t(1)), None);
        a.set_interpolation(&ch, t(1), Interpolation::Linear);
        assert_eq!(a.interpolation_at(&ch, t(1)), Some(Interpolation::Linear));
    }

    #[test]
    fn ron_round_trip() {
        let mut a = Animation::default();
        a.set_key(Channel::PositionX, Time::new(1, 30).unwrap(), 12.5);
        a.set_interpolation(&[Channel::PositionX], Time::new(1, 30).unwrap(), Interpolation::Hold);
        let text = ron::to_string(&a).unwrap();
        let back: Animation = ron::from_str(&text).unwrap();
        assert_eq!(a, back);
    }
}
