#![forbid(unsafe_code)]

use lumen_core::{
    Channel, Curve, Interpolation, Keyframe, Layer, LayerProps, Resolver, Time, Transform2D,
};

/// Value of `curve` at `time`, or `None` if it has no keyframes.
/// Before the first and after the last keyframe the curve is constant.
pub fn sample(curve: &Curve, time: Time) -> Option<f32> {
    match curve.segment(time) {
        (Some(a), Some(b)) => Some(interpolate(a, b, time)),
        (Some(a), None) => Some(a.value),
        (None, Some(b)) => Some(b.value),
        (None, None) => None,
    }
}

fn interpolate(a: &Keyframe, b: &Keyframe, time: Time) -> f32 {
    let secs = |from: Time, to: Time| to.checked_sub(from).map_or(0.0, Time::as_secs_f64);
    let span = secs(a.time, b.time);
    if span <= 0.0 {
        return a.value;
    }
    let s = (secs(a.time, time) / span).clamp(0.0, 1.0) as f32;
    match a.interpolation {
        Interpolation::Hold => a.value,
        Interpolation::Linear => a.value + (b.value - a.value) * s,
        Interpolation::Bezier => bezier_segment(a, b, s),
    }
}

fn cubic(p0: f32, p1: f32, p2: f32, p3: f32, u: f32) -> f32 {
    let v = 1.0 - u;
    v * v * v * p0 + 3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u * p3
}

fn cubic_derivative(p0: f32, p1: f32, p2: f32, p3: f32, u: f32) -> f32 {
    let v = 1.0 - u;
    3.0 * v * v * (p1 - p0) + 6.0 * v * u * (p2 - p1) + 3.0 * u * u * (p3 - p2)
}

/// Finds `u` with `x(u) == x` for a time curve with control points
/// `0, x1, x2, 1`. Monotonic as long as `x1, x2` lie in `0..=1`.
fn solve_time(x1: f32, x2: f32, x: f32) -> f32 {
    const EPS: f32 = 1e-6;
    let mut u = x;
    for _ in 0..8 {
        let err = cubic(0.0, x1, x2, 1.0, u) - x;
        if err.abs() < EPS {
            return u;
        }
        let d = cubic_derivative(0.0, x1, x2, 1.0, u);
        if d.abs() < EPS {
            break;
        }
        u = (u - err / d).clamp(0.0, 1.0);
    }
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    u = x;
    for _ in 0..40 {
        let xu = cubic(0.0, x1, x2, 1.0, u);
        if (xu - x).abs() < EPS {
            break;
        }
        if xu < x {
            lo = u;
        } else {
            hi = u;
        }
        u = 0.5 * (lo + hi);
    }
    u
}

/// Bezier segment from `a` to `b` at time fraction `s` (0..=1).
pub fn bezier_segment(a: &Keyframe, b: &Keyframe, s: f32) -> f32 {
    let x1 = a.out_tangent.dt.clamp(0.0, 1.0);
    let x2 = 1.0 - b.in_tangent.dt.clamp(0.0, 1.0);
    let u = solve_time(x1, x2, s.clamp(0.0, 1.0));
    cubic(
        a.value,
        a.value + a.out_tangent.dv,
        b.value + b.in_tangent.dv,
        b.value,
        u,
    )
}

/// Value of `channel` at `time`: the curve if animated, else the static value.
pub fn channel_value(props: &LayerProps, channel: Channel, time: Time) -> f32 {
    props
        .animation
        .curve(channel)
        .and_then(|c| sample(c, time))
        .unwrap_or_else(|| props.static_value(channel))
}

/// Transform and opacity of a layer at `time`.
pub fn evaluate(props: &LayerProps, time: Time) -> (Transform2D, f32) {
    let v = |ch| channel_value(props, ch, time);
    let mut t = props.transform.clone();
    t.position.x = v(Channel::PositionX);
    t.position.y = v(Channel::PositionY);
    t.scale.x = v(Channel::ScaleX);
    t.scale.y = v(Channel::ScaleY);
    t.rotation_deg = v(Channel::Rotation);
    (t, v(Channel::Opacity).clamp(0.0, 1.0))
}

fn value_of(t: &Transform2D, opacity: f32, channel: Channel) -> f32 {
    match channel {
        Channel::PositionX => t.position.x,
        Channel::PositionY => t.position.y,
        Channel::ScaleX => t.scale.x,
        Channel::ScaleY => t.scale.y,
        Channel::Rotation => t.rotation_deg,
        Channel::Opacity => opacity,
    }
}

/// Resolves layer values at a fixed point in time.
#[derive(Clone, Copy, Debug)]
pub struct AtTime(pub Time);

impl Resolver for AtTime {
    fn transform(&self, layer: &Layer) -> Transform2D {
        evaluate(&layer.props, self.0).0
    }

    fn opacity(&self, layer: &Layer) -> f32 {
        evaluate(&layer.props, self.0).1
    }
}

/// Writes `value` to `channel` at `time`.
///
/// Animated channels get a keyframe. Static channels change their stored
/// value, unless `record` is set, in which case they start animating.
pub fn write_channel(
    props: &mut LayerProps,
    channel: Channel,
    time: Time,
    value: f32,
    record: bool,
) {
    let value = if channel == Channel::Opacity {
        value.clamp(0.0, 1.0)
    } else {
        value
    };
    if props.animation.is_animated(channel) || record {
        props.animation.set_key(channel, time, value);
    } else {
        props.set_static_value(channel, value);
    }
}

/// Applies an edit from `before` to `after` (both evaluated at `time`),
/// touching only the channels whose value actually changed.
pub fn write_transform(
    props: &mut LayerProps,
    before: (&Transform2D, f32),
    after: (&Transform2D, f32),
    time: Time,
    record: bool,
) {
    for channel in Channel::ALL {
        let old = value_of(before.0, before.1, channel);
        let new = value_of(after.0, after.1, channel);
        if (new - old).abs() > 1e-6 {
            write_channel(props, channel, time, new, record);
        }
    }
    props.transform.anchor = after.0.anchor;
}

/// Removes the keyframes at `time` from `channels`. A channel that loses its
/// last keyframe keeps the value it had at `time` as its static value.
pub fn remove_keys(props: &mut LayerProps, channels: &[Channel], time: Time) {
    for ch in channels {
        if !props.animation.has_key(*ch, time) {
            continue;
        }
        let value = channel_value(props, *ch, time);
        props.animation.remove_key(*ch, time);
        if !props.animation.is_animated(*ch) {
            props.set_static_value(*ch, value);
        }
    }
}

/// Adds keyframes at `time` for all `channels` (using their current values),
/// or removes them if every channel already has one there.
/// Returns `true` if keyframes were added.
pub fn toggle_keys(props: &mut LayerProps, channels: &[Channel], time: Time) -> bool {
    let all_keyed = channels.iter().all(|ch| props.animation.has_key(*ch, time));
    if all_keyed {
        remove_keys(props, channels, time);
        false
    } else {
        let values: Vec<f32> = channels
            .iter()
            .map(|ch| channel_value(props, *ch, time))
            .collect();
        for (ch, v) in channels.iter().zip(values) {
            props.animation.set_key(*ch, time, v);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::glam::Vec2;
    use lumen_core::{FrameRate, LayerKind, Tangent};

    fn t(secs: i64) -> Time {
        Time::from_secs(secs)
    }

    fn curve(keys: &[(i64, f32, Interpolation)]) -> Curve {
        let mut c = Curve::default();
        for &(s, v, i) in keys {
            let mut k = Keyframe::new(t(s), v);
            k.interpolation = i;
            c.insert(k);
        }
        c
    }

    fn rect() -> LayerProps {
        LayerProps::new(
            "r",
            LayerKind::Rectangle {
                size: Vec2::splat(10.0),
                color: [1.0; 4],
                corner_radius: 0.0,
            },
        )
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn empty_curve_has_no_value() {
        assert_eq!(sample(&Curve::default(), t(0)), None);
    }

    #[test]
    fn constant_outside_keys() {
        let c = curve(&[(1, 10.0, Interpolation::Linear), (3, 30.0, Interpolation::Linear)]);
        assert_eq!(sample(&c, t(0)), Some(10.0));
        assert_eq!(sample(&c, t(5)), Some(30.0));
        assert_eq!(sample(&c, t(3)), Some(30.0));
    }

    #[test]
    fn linear_and_hold() {
        let c = curve(&[(0, 0.0, Interpolation::Linear), (2, 100.0, Interpolation::Linear)]);
        assert!(close(sample(&c, t(1)).unwrap(), 50.0));
        assert!(close(sample(&c, Time::new(1, 2).unwrap()).unwrap(), 25.0));

        let h = curve(&[(0, 0.0, Interpolation::Hold), (2, 100.0, Interpolation::Linear)]);
        assert_eq!(sample(&h, Time::new(19, 10).unwrap()), Some(0.0));
        assert_eq!(sample(&h, t(2)), Some(100.0));
    }

    #[test]
    fn bezier_ease_is_symmetric_and_slow_at_ends() {
        let c = curve(&[(0, 0.0, Interpolation::Bezier), (1, 100.0, Interpolation::Bezier)]);
        let mid = sample(&c, Time::new(1, 2).unwrap()).unwrap();
        assert!(close(mid, 50.0), "mid = {mid}");
        let early = sample(&c, Time::new(1, 10).unwrap()).unwrap();
        assert!(early < 10.0, "ease-in should lag behind linear, got {early}");
        let late = sample(&c, Time::new(9, 10).unwrap()).unwrap();
        assert!(close(early + late, 100.0));
    }

    #[test]
    fn bezier_with_straight_handles_matches_linear() {
        let mut a = Keyframe::new(t(0), 0.0);
        let mut b = Keyframe::new(t(1), 90.0);
        a.out_tangent = Tangent { dt: 1.0 / 3.0, dv: 30.0 };
        b.in_tangent = Tangent { dt: 1.0 / 3.0, dv: -30.0 };
        for i in 0..=10 {
            let s = i as f32 / 10.0;
            assert!(close(bezier_segment(&a, &b, s), 90.0 * s), "s = {s}");
        }
    }

    #[test]
    fn solve_time_handles_extreme_handles() {
        for (x1, x2) in [(0.0, 1.0), (1.0, 0.0), (0.0, 0.0), (1.0, 1.0)] {
            for i in 0..=20 {
                let x = i as f32 / 20.0;
                let u = solve_time(x1, x2, x);
                assert!((cubic(0.0, x1, x2, 1.0, u) - x).abs() < 1e-3, "x1={x1} x2={x2} x={x}");
            }
        }
    }

    #[test]
    fn ntsc_keyframes_hit_exact_values() {
        let rate = FrameRate::FPS_29_97;
        let mut c = Curve::default();
        let mut k0 = Keyframe::new(Time::from_frame(0, rate).unwrap(), 0.0);
        k0.interpolation = Interpolation::Linear;
        c.insert(k0);
        c.insert(Keyframe::new(Time::from_frame(30, rate).unwrap(), 30.0));
        let v = sample(&c, Time::from_frame(15, rate).unwrap()).unwrap();
        assert!(close(v, 15.0));
    }

    #[test]
    fn evaluate_mixes_static_and_animated() {
        let mut p = rect();
        p.transform.position = Vec2::new(5.0, 7.0);
        p.animation.set_key(Channel::PositionX, t(0), 0.0);
        p.animation.set_key(Channel::PositionX, t(2), 200.0);
        p.animation
            .set_interpolation(&[Channel::PositionX], t(0), Interpolation::Linear);
        let (tr, opacity) = evaluate(&p, t(1));
        assert!(close(tr.position.x, 100.0));
        assert!(close(tr.position.y, 7.0));
        assert!(close(opacity, 1.0));
    }

    #[test]
    fn write_channel_respects_animation_and_record() {
        let mut p = rect();
        write_channel(&mut p, Channel::Rotation, t(1), 45.0, false);
        assert_eq!(p.transform.rotation_deg, 45.0);
        assert!(!p.animation.is_animated(Channel::Rotation));

        write_channel(&mut p, Channel::Rotation, t(1), 90.0, true);
        assert!(p.animation.has_key(Channel::Rotation, t(1)));

        write_channel(&mut p, Channel::Rotation, t(2), 180.0, false);
        assert!(p.animation.has_key(Channel::Rotation, t(2)), "animated channels always key");

        write_channel(&mut p, Channel::Opacity, t(0), 3.0, false);
        assert_eq!(p.opacity, 1.0);
    }

    #[test]
    fn write_transform_only_touches_changed_channels() {
        let mut p = rect();
        let (before, op) = evaluate(&p, t(1));
        let mut after = before.clone();
        after.position.x = 50.0;
        write_transform(&mut p, (&before, op), (&after, op), t(1), true);
        assert!(p.animation.is_animated(Channel::PositionX));
        assert!(!p.animation.is_animated(Channel::PositionY));
        assert!(!p.animation.is_animated(Channel::Rotation));
    }

    #[test]
    fn toggle_keys_adds_then_removes() {
        let mut p = rect();
        p.transform.scale = Vec2::new(2.0, 3.0);
        let ch = [Channel::ScaleX, Channel::ScaleY];
        assert!(toggle_keys(&mut p, &ch, t(1)));
        assert_eq!(p.animation.curve(Channel::ScaleY).unwrap().keys()[0].value, 3.0);
        assert!(!toggle_keys(&mut p, &ch, t(1)));
        assert!(p.animation.is_empty());
        assert_eq!(p.transform.scale, Vec2::new(2.0, 3.0));
    }

    #[test]
    fn removing_last_key_keeps_current_value() {
        let mut p = rect();
        p.animation.set_key(Channel::Rotation, t(0), 30.0);
        toggle_keys(&mut p, &[Channel::Rotation], t(0));
        assert_eq!(p.transform.rotation_deg, 30.0);
    }

    #[test]
    fn resolver_drives_scene_evaluation() {
        let mut project = lumen_core::Project::default();
        let mut history = lumen_core::History::default();
        let id = project.scene.allocate_id();
        let mut p = rect();
        p.animation.set_key(Channel::PositionX, t(0), 0.0);
        p.animation.set_key(Channel::PositionX, t(1), 100.0);
        history
            .execute(&mut project, Box::new(lumen_core::AddLayer::new(id, None, 0, p)))
            .unwrap();

        let items = project.scene.flatten_with(&AtTime(t(1)));
        assert!(close(items[0].world.translation.x, 100.0));
        let probe = Vec2::new(100.0, 0.0);
        assert!(project.scene.hit_test_with(probe, &AtTime(t(1))).is_some());
        assert!(project.scene.hit_test_with(probe, &AtTime(t(0))).is_none());
    }
}
