use super::*;

fn lcg(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

/// Random track with strictly increasing times and mixed tangent kinds.
fn random_track(seed: &mut u64, n: usize, kinds: &[Tan]) -> Track {
    let mut t = 0.0;
    let keys = (0..n)
        .map(|_| {
            t += 0.3 + lcg(seed) * 2.0;
            let kind = kinds[(lcg(seed) * kinds.len() as f64) as usize % kinds.len()];
            Key::new(t, lcg(seed) * 10.0 - 5.0).with_tan(kind)
        })
        .collect();
    Track::from_keys(keys).unwrap()
}

fn tr(pts: &[(f64, f64)], tan: Tan) -> Track {
    Track::from_keys(
        pts.iter()
            .map(|&(t, v)| Key::new(t, v).with_tan(tan))
            .collect(),
    )
    .unwrap()
}

#[test]
fn passes_through_every_key_and_is_c0() {
    let mut seed = 1;
    for _ in 0..100 {
        let t = random_track(
            &mut seed,
            6,
            &[Tan::Smooth, Tan::Linear, Tan::Flat, Tan::CatmullRom],
        );
        for k in t.keys() {
            assert!((t.eval(k.t()) - k.v()).abs() < 1e-12);
            assert!((t.eval(k.t() - 1e-9) - k.v()).abs() < 1e-6);
            assert!((t.eval(k.t() + 1e-9) - k.v()).abs() < 1e-6);
        }
    }
}

#[test]
fn unbroken_smooth_keys_are_c1() {
    let mut seed = 2;
    let t = random_track(&mut seed, 8, &[Tan::Smooth]);
    let h = 1e-6;
    for k in &t.keys()[1..7] {
        let d0 = (t.eval(k.t()) - t.eval(k.t() - h)) / h;
        let d1 = (t.eval(k.t() + h) - t.eval(k.t())) / h;
        assert!((d0 - d1).abs() < 1e-4, "slopes {d0} vs {d1}");
    }
}

#[test]
fn smooth_never_leaves_neighbour_range_catmull_does() {
    let mut seed = 3;
    for _ in 0..100 {
        let t = random_track(&mut seed, 7, &[Tan::Smooth]);
        for w in t.keys().windows(2) {
            let (lo, hi) = (w[0].v().min(w[1].v()), w[0].v().max(w[1].v()));
            for s in 0..=50 {
                let x = w[0].t() + (w[1].t() - w[0].t()) * f64::from(s) / 50.0;
                let v = t.eval(x);
                assert!(v >= lo - 1e-9 && v <= hi + 1e-9, "{v} outside [{lo},{hi}]");
            }
        }
    }
    let pts = [(0.0, 0.0), (1.0, 0.0), (2.0, 1.0), (3.0, 1.0)];
    let cr = tr(&pts, Tan::CatmullRom);
    assert!(
        (0..=100).any(|s| {
            let v = cr.eval(1.0 + f64::from(s) / 100.0);
            !(0.0..=1.0).contains(&v)
        }) || (0..=100).any(|s| !(0.0..=1.0).contains(&cr.eval(f64::from(s) / 100.0)))
    );
}

#[test]
fn ramp_on_unit_interval_stays_in_range_and_monotone() {
    // Colour-like ramp: three channels over [0,1], values in [0,1].
    let stops = [
        (0.0f32, [0.0f32, 0.2, 1.0]),
        (0.15, [0.1, 0.2, 0.9]),
        (0.6, [0.9, 0.8, 0.1]),
        (1.0, [1.0, 0.8, 0.0]),
    ];
    let mut ch = Channels::<f32, 3>::default();
    for c in 0..3 {
        for &(p, rgb) in &stops {
            ch.track_mut(c)
                .unwrap()
                .add(p, rgb[c], Tan::Smooth)
                .unwrap();
        }
    }
    let mut prev = ch.eval(0.0);
    for s in 1..=1000 {
        let v = ch.eval(s as f32 / 1000.0);
        for (c, x) in v.iter().enumerate() {
            assert!((-1e-6..=1.0 + 1e-6).contains(x), "channel {c} = {x}");
        }
        // Channels 0 and 1 rise, channel 2 falls, all monotonically (the data is monotone).
        assert!(v[0] >= prev[0] - 1e-6 && v[2] <= prev[2] + 1e-6);
        prev = v;
    }
    assert_eq!(ch.eval(-1.0), [0.0, 0.2, 1.0]);
    assert_eq!(ch.eval(2.0), [1.0, 0.8, 0.0]);
}

#[test]
fn heavy_weights_keep_eval_monotone() {
    let mut seed = 4;
    for _ in 0..50 {
        let mut t = 0.0;
        let mut v = 0.0;
        let mut track = Track::new();
        for _ in 0..6 {
            t += 0.5 + lcg(&mut seed);
            v += lcg(&mut seed);
            let mut k = Key::new(t, v);
            k.inn = Side::user(0.0, Some(lcg(&mut seed) * 100.0));
            k.out = Side::user(0.0, Some(lcg(&mut seed) * 100.0));
            k.broken = true;
            track.insert(k).unwrap();
        }
        let mut prev = track.eval(0.0);
        let (a, b) = track.range().unwrap();
        for s in 0..=2000 {
            let x = a + (b - a) * f64::from(s) / 2000.0;
            let y = track.eval(x);
            assert!(y >= prev - 1e-9, "not monotone at {x}");
            prev = y;
        }
    }
}

#[test]
fn newton_matches_bisection() {
    let mut seed = 5;
    for _ in 0..2000 {
        let (a, b, tau) = (lcg(&mut seed), lcg(&mut seed), lcg(&mut seed));
        let x = |u: f64| bez(0.0, a, 1.0 - b, 1.0, u);
        let u = solve_u(a, b, tau);
        assert!((x(u) - tau).abs() < 1e-6, "a={a} b={b} tau={tau}");
    }
}

#[test]
fn holds() {
    // out Constant: left value until the next key.
    let mut t = tr(&[(0.0, 1.0), (1.0, 5.0), (2.0, 2.0)], Tan::Smooth);
    t.set_side(0, SideId::Out, Side::new(Tan::Constant))
        .unwrap();
    assert_eq!((t.eval(0.0), t.eval(0.99), t.eval(1.0)), (1.0, 1.0, 5.0));
    // in Constant on the right key: jump right after the left key.
    let mut t = tr(&[(0.0, 1.0), (1.0, 5.0), (2.0, 2.0)], Tan::Smooth);
    t.set_side(1, SideId::In, Side::new(Tan::Constant)).unwrap();
    assert_eq!(
        (t.eval(0.0), t.eval(1e-9), t.eval(0.5), t.eval(1.0)),
        (1.0, 5.0, 5.0, 5.0)
    );
    // both: out wins.
    t.set_seg(0, Tan::Constant).unwrap();
    assert_eq!(t.eval(0.5), 1.0);
}

#[test]
fn extrapolation_modes() {
    let mut t = tr(&[(0.0, 0.0), (1.0, 2.0), (2.0, 3.0)], Tan::Linear);
    assert_eq!((t.eval(-5.0), t.eval(9.0)), (0.0, 3.0));
    t.set_extrap(Extrap::Linear, Extrap::Linear);
    assert!((t.eval(-1.0) + 2.0).abs() < 1e-12 && (t.eval(4.0) - 5.0).abs() < 1e-12);
    t.set_extrap(Extrap::Cycle, Extrap::Cycle);
    assert!(
        (t.eval(2.5) - t.eval(0.5)).abs() < 1e-12 && (t.eval(-0.5) - t.eval(1.5)).abs() < 1e-12
    );
    t.set_extrap(Extrap::CycleOffset, Extrap::CycleOffset);
    assert!((t.eval(2.5) - (t.eval(0.5) + 3.0)).abs() < 1e-12);
    assert!((t.eval(-0.5) - (t.eval(1.5) - 3.0)).abs() < 1e-12);
    t.set_extrap(Extrap::PingPong, Extrap::PingPong);
    assert!((t.eval(2.5) - t.eval(1.5)).abs() < 1e-12 && (t.eval(4.5) - t.eval(0.5)).abs() < 1e-12);
    assert!((t.eval(-0.5) - t.eval(0.5)).abs() < 1e-12);
    // A Constant side gives zero outer slope for Linear extrapolation.
    t.set_side(2, SideId::In, Side::new(Tan::Constant)).unwrap();
    t.set_extrap(Extrap::Linear, Extrap::Linear);
    assert_eq!(t.eval(5.0), 3.0);
    // Degenerate tracks.
    let mut one = Track::new();
    one.add(1.0, 7.0, Tan::Smooth).unwrap();
    one.set_extrap(Extrap::Cycle, Extrap::PingPong);
    assert_eq!(
        (Track::<f64>::new().eval(3.0), one.eval(-4.0), one.eval(4.0)),
        (0.0, 7.0, 7.0)
    );
    assert!(t.eval(f64::NAN).is_nan());
}

#[test]
fn editing_keeps_cache_equal_to_a_fresh_build() {
    // Local splices (insert, remove, move, value/tangent edits, shift, auto-key) must equal a full rebuild.
    let mut seed = 6;
    let mut t = random_track(&mut seed, 8, &[Tan::Smooth, Tan::CatmullRom, Tan::Linear]);
    let tans = [
        Tan::Smooth,
        Tan::Flat,
        Tan::Linear,
        Tan::Constant,
        Tan::CatmullRom,
    ];
    for step in 0..1500usize {
        let n = t.len();
        let i = (lcg(&mut seed) * n as f64) as usize % n.max(1);
        let rt = lcg(&mut seed) * 40.0 - 5.0;
        let tan = tans[step % tans.len()];
        match step % 11 {
            0 => t.set_value(i, lcg(&mut seed) * 4.0).unwrap(),
            1 => t.set_tan(i, tan).unwrap(),
            2 => t
                .set_user(i, SideId::Out, lcg(&mut seed) - 0.5, Some(lcg(&mut seed)))
                .unwrap(),
            3 => t.set_broken(i, step % 2 == 0).unwrap(),
            4 if n > 1 => t.set_seg(i.min(n - 2), tan).unwrap(),
            5 | 6 => {
                t.add(rt, lcg(&mut seed) * 4.0, tan).unwrap();
            }
            7 if n > 1 => {
                t.remove(i).unwrap();
            }
            8 => {
                let at = t.move_key(i, rt).unwrap();
                assert_eq!(t.keys()[at].t(), rt);
            }
            9 if n > 2 => {
                // May fail on a collision; the track must then be unchanged.
                let before = t.clone();
                if t.shift(&[i, (i + 1) % n], lcg(&mut seed) * 4.0 - 2.0)
                    .is_err()
                {
                    assert_eq!(t, before);
                }
            }
            10 => {
                t.set_at(rt, 1.0, tan).unwrap();
            }
            _ => {
                t.add(rt, 0.5, tan).unwrap();
            }
        }
        if t.is_empty() {
            t.add(0.0, 0.0, Tan::Smooth).unwrap();
        }
        let fresh = Track::from_keys(t.keys().to_vec()).unwrap();
        assert_eq!(t, fresh, "local splice diverged at step {step}");
        assert!(t.keys().windows(2).all(|w| w[0].t() < w[1].t()));
    }
}

#[test]
fn unbroken_coupling_and_set_seg() {
    let mut t = tr(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)], Tan::Smooth);
    t.set_user(1, SideId::Out, 0.5, Some(0.2)).unwrap();
    let k = t.keys()[1];
    assert_eq!(
        (k.inn.kind, k.inn.slope, k.inn.weight),
        (Tan::User, 0.5, None)
    );
    t.set_broken(1, true).unwrap();
    t.set_user(1, SideId::In, -2.0, None).unwrap();
    assert_eq!((t.keys()[1].out.slope, t.keys()[1].inn.slope), (0.5, -2.0));
    // A linear segment between smooth keys is a straight line and breaks only the keys that differ.
    let mut t = tr(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 3.0), (3.0, 3.0)],
        Tan::Smooth,
    );
    t.set_seg(1, Tan::Linear).unwrap();
    assert!(t.keys()[1].broken && t.keys()[2].broken && !t.keys()[0].broken);
    for s in 0..=10 {
        let x = 1.0 + f64::from(s) / 10.0;
        assert!((t.eval(x) - (1.0 + 2.0 * (x - 1.0))).abs() < 1e-12);
    }
}

#[test]
fn insert_upserts_and_validates() {
    let mut t = Track::new();
    assert_eq!(t.add(1.0, 1.0, Tan::Smooth), Ok(0));
    assert_eq!(t.add(0.0, 0.0, Tan::Smooth), Ok(0));
    assert_eq!(t.add(1.0, 9.0, Tan::Linear), Ok(1));
    assert_eq!((t.len(), t.eval(1.0)), (2, 9.0));
    assert_eq!(t.add(f64::NAN, 0.0, Tan::Smooth), Err(KeysError::NotFinite));
    assert_eq!(t.move_key(0, 1.0), Err(KeysError::Order(1)));
    assert_eq!(t.remove(5).unwrap_err(), KeysError::Index);
    assert_eq!(
        Track::from_keys(vec![Key::new(1.0, 0.0), Key::new(1.0, 1.0)]).unwrap_err(),
        KeysError::Order(1)
    );
    // A key moved past its neighbour reorders.
    assert_eq!(t.move_key(0, 5.0), Ok(1));
}

#[test]
fn handles_report_effective_handles() {
    let t = tr(&[(0.0, 0.0), (3.0, 3.0)], Tan::Linear);
    // The last key has no out segment, the first no in segment: those sides are `None`.
    assert_eq!(t.handle(1, SideId::In), Some((-1.0, -1.0)));
    assert_eq!(t.handle(1, SideId::Out), None);
    assert_eq!(t.handle(0, SideId::In), None);
    assert_eq!(t.handle(0, SideId::Out), Some((1.0, 1.0)));
    assert!(t.handle(2, SideId::In).is_none());
}

#[test]
fn scale_time_keeps_the_shape() {
    let mut seed = 8;
    let mut t = random_track(&mut seed, 6, &[Tan::Smooth, Tan::CatmullRom, Tan::Flat]);
    t.set_user(2, SideId::Out, 0.7, Some(0.3)).unwrap();
    let old = t.clone();
    t.scale_time(24.0).unwrap();
    for s in 0..=200 {
        let x = f64::from(s) * 0.05;
        assert!((old.eval(x) - t.eval(x * 24.0)).abs() < 1e-9);
    }
    assert_eq!(t.scale_time(0.0), Err(KeysError::Scale));
}

#[test]
fn hint_and_bulk_equal_plain_eval() {
    let mut seed = 9;
    let t = random_track(&mut seed, 10, &[Tan::Smooth, Tan::Linear]);
    let mut buf = vec![0.0; 400];
    t.eval_many(-1.0, 0.07, &mut buf);
    let mut hint = 3;
    for (k, b) in buf.iter().enumerate() {
        let x = -1.0 + 0.07 * k as f64;
        assert_eq!(*b, t.eval(x));
        assert_eq!(t.eval_hint(x, &mut hint), t.eval(x));
    }
    // A stale hint from another position is only a hint.
    let mut stale = 99;
    assert_eq!(t.eval_hint(1.0, &mut stale), t.eval(1.0));
}

#[test]
fn unwrap_rotations() {
    let mut v = [350.0, 10.0, 30.0, 350.0, 5.0];
    unwind_slice(&mut v, 360.0).unwrap();
    assert_eq!(v, [350.0, 370.0, 390.0, 350.0, 365.0]);
    let mut t = Track::new();
    for (k, a) in [170.0, -170.0, -150.0].iter().enumerate() {
        t.add(k as f64, *a, Tan::Smooth).unwrap();
    }
    t.unwind(360.0).unwrap();
    assert_eq!(t.keys()[1].v(), 190.0);
}

#[test]
fn f32_tracks_work() {
    let mut t = Track::<f32>::new();
    for k in 0..5u8 {
        t.add(f32::from(k), f32::from(k * k), Tan::Smooth).unwrap();
    }
    assert!((t.eval(2.0) - 4.0).abs() < 1e-6);
}

#[cfg(feature = "serde")]
#[test]
fn serde_round_trip_and_defaults() {
    let mut t = tr(&[(0.0, 1.0), (1.0, 2.0), (2.0, 0.0)], Tan::Smooth);
    t.set_broken(1, true).unwrap();
    t.set_user(1, SideId::Out, 0.5, Some(0.2)).unwrap();
    t.set_seg(0, Tan::Constant).unwrap();
    t.set_extrap(Extrap::Cycle, Extrap::PingPong);
    let j = serde_json::to_string(&t).unwrap();
    let back: Track = serde_json::from_str(&j).unwrap();
    assert_eq!(t, back);
    assert!(!j.contains("\"weight\":null"), "{j}");
    // A bare key is a valid Smooth key; extrapolation defaults to constant.
    let b: Track = serde_json::from_str(r#"{"keys":[{"t":0,"v":1},{"t":1,"v":3}]}"#).unwrap();
    assert_eq!(
        (b.keys()[0].out.kind, b.pre(), b.eval(0.5)),
        (Tan::Smooth, Extrap::Constant, 2.0)
    );
    // Invalid order is a deserialization error.
    assert!(serde_json::from_str::<Track>(r#"{"keys":[{"t":1,"v":0},{"t":0,"v":1}]}"#).is_err());
    let ch = Channels::<f64, 2>::new([t.clone(), t]);
    let json = serde_json::to_string(&ch).unwrap();
    let back: Channels<f64, 2> = serde_json::from_str(&json).unwrap();
    assert_eq!(ch, back);
    assert!(serde_json::from_str::<Channels<f64, 3>>(&json).is_err());
}

// ---- review fixes ----

#[test]
fn infinite_and_huge_times_never_panic() {
    let tracks: [&[(f64, f64)]; 3] = [&[(0.0, 1.0), (1.0, 3.0), (2.5, 2.0)], &[(1.0, 7.0)], &[]];
    for pre in Extrap::ALL {
        for post in Extrap::ALL {
            for pts in tracks {
                let mut t = tr(pts, Tan::Smooth);
                t.set_extrap(pre, post);
                // An empty track evaluates to 0 everywhere.
                let first = pts.first().map_or(0.0, |p| p.1);
                let last = pts.last().map_or(0.0, |p| p.1);
                for x in [f64::INFINITY, 1e300, 1.7e308, f64::MAX] {
                    assert!(!t.eval(x).is_nan() && !t.eval(-x).is_nan());
                    assert!(!t.slope_at(x).is_nan() && !t.slope_at(-x).is_nan());
                }
                assert_eq!(t.eval(f64::INFINITY), last, "{pre:?}/{post:?}");
                assert_eq!(t.eval(f64::NEG_INFINITY), first, "{pre:?}/{post:?}");
                let mut buf = [0.0; 3];
                t.eval_many(f64::NEG_INFINITY, f64::INFINITY, &mut buf);
            }
        }
    }
    // f32: the period quotient overflows long before f32::MAX.
    let mut t = Track::<f32>::new();
    t.add(0.0, 0.0, Tan::Linear).unwrap();
    t.add(1e-30, 1.0, Tan::Linear).unwrap();
    t.set_extrap(Extrap::PingPong, Extrap::CycleOffset);
    for x in [3e38f32, -3e38, f32::INFINITY] {
        assert!(!t.eval(x).is_nan());
    }
}

#[test]
fn negative_zero_is_one_time() {
    let mut t = Track::<f64>::new();
    assert_eq!(t.add(0.0, 1.0, Tan::Smooth), Ok(0));
    assert_eq!(t.add(-0.0, 2.0, Tan::Smooth), Ok(0));
    assert_eq!((t.len(), t.eval(0.0)), (1, 2.0));
    assert!(t.keys()[0].t().is_sign_positive());
    assert_eq!(t.find(-0.0), Ok(0));
    t.add(1.0, 0.0, Tan::Smooth).unwrap();
    // Moving onto -0.0 collides with the key at 0.0.
    assert_eq!(t.move_key(1, -0.0), Err(KeysError::Order(0)));
    assert_eq!(
        Track::from_keys(vec![Key::new(0.0, 0.0), Key::new(-0.0, 1.0)]).unwrap_err(),
        KeysError::Order(1)
    );
    let u = Track::from_keys(vec![Key::new(-0.0f64, 0.0), Key::new(1.0, 1.0)]).unwrap();
    assert!(u.keys()[0].t().is_sign_positive());
}

#[test]
fn non_finite_and_negative_handles_are_rejected() {
    let mut t = tr(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)], Tan::Smooth);
    let before = t.clone();
    for bad in [f64::NAN, f64::INFINITY] {
        let e = Err(KeysError::NotFinite);
        assert_eq!(t.set_user(1, SideId::Out, bad, None), e);
        assert_eq!(t.set_user(1, SideId::In, 0.0, Some(bad)), e);
        assert_eq!(t.set_value(1, bad), e);
        assert_eq!(t.set_handle(1, SideId::Out, (1.0, bad)), e);
        assert_eq!(t.shift(&[1], bad), e);
        let mut k = Key::new(5.0, 0.0);
        k.out = Side::user(bad, None);
        assert_eq!(Track::from_keys(vec![k]).unwrap_err(), KeysError::NotFinite);
        assert_eq!(t.insert(k), e.map(|()| 0));
    }
    assert_eq!(
        t.set_user(1, SideId::Out, 0.0, Some(-0.1)),
        Err(KeysError::Weight)
    );
    assert_eq!(t, before);
    // An auto side drops a stray slope instead of failing.
    let mut k = Key::new(5.0, 0.0);
    k.out.slope = f64::NAN;
    assert!(Track::from_keys(vec![k]).is_ok());
}

#[cfg(feature = "serde")]
#[test]
fn serde_rejects_bad_handles_and_unifies_zero() {
    assert!(
        serde_json::from_str::<Track>(
            r#"{"keys":[{"t":0,"v":0,"out":{"kind":"user","weight":-1}},{"t":1,"v":1}]}"#
        )
        .is_err()
    );
    let t: Track = serde_json::from_str(r#"{"keys":[{"t":-0.0,"v":0},{"t":1,"v":1}]}"#).unwrap();
    let back: Track = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
    assert_eq!(t, back);
}

#[test]
fn unwind_validates_and_rolls_back() {
    let mut t = tr(&[(0.0, 350.0), (1.0, 10.0), (2.0, 30.0)], Tan::Smooth);
    let before = t.clone();
    for p in [0.0, -360.0, f64::NAN, f64::INFINITY] {
        assert_eq!(t.unwind(p), Err(KeysError::Scale));
        let mut v = [1.0, 2.0];
        assert_eq!(unwind_slice(&mut v, p), Err(KeysError::Scale));
        assert_eq!(v, [1.0, 2.0]);
    }
    assert_eq!(t, before);
    t.unwind(360.0).unwrap();
    assert_eq!(t.keys()[1].v(), 370.0);
    // Overflow of the running offset: error, nothing written.
    let mut v = [0.0f32, 3e38, -3e38];
    assert_eq!(unwind_slice(&mut v, 1e-30), Err(KeysError::NotFinite));
    assert_eq!(v, [0.0, 3e38, -3e38]);
}

#[test]
fn scale_time_validates_and_rolls_back() {
    let mut t = tr(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)], Tan::Smooth);
    t.set_user(1, SideId::Out, 1.0, Some(0.2)).unwrap();
    let before = t.clone();
    for k in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(t.scale_time(k), Err(KeysError::Scale));
    }
    // Overflow of a time and collapsing times: error, unchanged.
    assert_eq!(t.scale_time(f64::MAX), Err(KeysError::NotFinite));
    assert!(t.scale_time(1e-320).is_err());
    assert_eq!(t, before);
    let mut u = Track::<f32>::new();
    u.add(1.0, 0.0, Tan::Linear).unwrap();
    u.add(1.000_000_1, 1.0, Tan::Linear).unwrap();
    let before_u = u.clone();
    assert_eq!(u.scale_time(1e-39), Err(KeysError::Order(1)));
    assert_eq!(u, before_u);
}

#[test]
fn constant_applies_to_the_out_side_only() {
    let mut t = tr(&[(0.0, 0.0), (1.0, 4.0), (2.0, 1.0)], Tan::Linear);
    t.set_tan(1, Tan::Constant).unwrap();
    let k = t.keys()[1];
    assert_eq!((k.inn.kind, k.out.kind), (Tan::Linear, Tan::Constant));
    // The segment arriving at key 1 is still a straight line, the one leaving it is a hold.
    assert!((t.eval(0.5) - 2.0).abs() < 1e-12);
    assert_eq!((t.eval(1.5), t.eval(2.0)), (4.0, 1.0));
    // set_side(Out, Constant) on an unbroken key does not touch the in side either.
    let mut t = tr(&[(0.0, 0.0), (1.0, 4.0), (2.0, 1.0)], Tan::Smooth);
    t.set_side(1, SideId::Out, Side::new(Tan::Constant))
        .unwrap();
    assert_eq!(t.keys()[1].inn.kind, Tan::Smooth);
    assert_eq!(t.keys()[1].with_tan(Tan::Constant).inn.kind, Tan::Smooth);
    // Step-in is explicit and survives editing the out side.
    t.set_side(1, SideId::In, Side::new(Tan::Constant)).unwrap();
    t.set_side(1, SideId::Out, Side::new(Tan::Flat)).unwrap();
    assert_eq!(t.keys()[1].inn.kind, Tan::Constant);
    assert_eq!(t.eval(1e-9), 4.0);
    // add(Constant) is a step out.
    let mut t = Track::new();
    t.add(0.0, 1.0, Tan::Constant).unwrap();
    t.add(1.0, 2.0, Tan::Constant).unwrap();
    assert_eq!((t.eval(0.9), t.eval(1.0)), (1.0, 2.0));
}

#[test]
fn newton_corner_weights_are_solved_to_tolerance() {
    // Degenerate widths: 0, 1, near 1/2, tiny and huge ratios, over many tau.
    let ws = [0.0, 1e-9, 0.01, 1.0 / 3.0, 0.5, 0.99, 1.0 - 1e-9, 1.0];
    for &a in &ws {
        for &b in &ws {
            for s in 0..=200 {
                let tau = f64::from(s) / 200.0;
                let u = solve_u(a, b, tau);
                let x = bez(0.0, a, 1.0 - b, 1.0, u);
                assert!(
                    (x - tau).abs() < 1e-12,
                    "a={a} b={b} tau={tau}: |X-tau|={}",
                    (x - tau).abs()
                );
                assert!((0.0..=1.0).contains(&u));
            }
        }
    }
    // The old 8-iteration solver was off by a few percent in value near here (a = 0, b ~ 1).
    let mut k0 = Key::new(0.0, 0.0);
    k0.broken = true;
    k0.out = Side::user(0.0, Some(0.0));
    let mut k1 = Key::new(1.0, 1.0);
    k1.broken = true;
    k1.inn = Side::user(0.0, Some(0.999));
    let t = Track::from_keys(vec![k0, k1]).unwrap();
    let mut prev = t.eval(0.0);
    for s in 1..=1000 {
        let v = t.eval(f64::from(s) * 1e-3);
        assert!(v >= prev - 1e-12 && (0.0..=1.0).contains(&v));
        prev = v;
    }
    // f32 corner solve too.
    for s in 0..=100 {
        let tau = s as f32 / 100.0;
        let u = solve_u(0.0f32, 1.0, tau);
        assert!((bez(0.0, 0.0, 0.0, 1.0, u) - tau).abs() < 1e-5);
    }
}

#[test]
fn single_key_linear_extrapolation_uses_the_sides() {
    let mut t = Track::new();
    let mut k = Key::new(1.0, 5.0);
    k.broken = true;
    k.inn = Side::user(2.0, None);
    k.out = Side::user(-1.0, None);
    t.insert(k).unwrap();
    t.set_extrap(Extrap::Linear, Extrap::Linear);
    assert_eq!(t.eval(1.0), 5.0);
    assert_eq!(t.eval(3.0), 5.0 + 2.0 * 2.0); // post: the in slope (Nuke rule)
    assert_eq!(t.eval(-1.0), 5.0 + (-1.0) * (-2.0)); // pre: the out slope
    t.set_extrap(Extrap::Cycle, Extrap::PingPong);
    assert_eq!((t.eval(-9.0), t.eval(9.0)), (5.0, 5.0));
}

#[test]
fn find_and_set_at() {
    let mut t = tr(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)], Tan::Smooth);
    assert_eq!(
        (t.find(1.0), t.find(1.5), t.find(-1.0), t.find(9.0)),
        (Ok(1), Err(2), Err(0), Err(3))
    );
    t.set_user(1, SideId::Out, 0.7, Some(0.25)).unwrap();
    t.set_broken(1, true).unwrap();
    t.set_user(1, SideId::In, -0.3, None).unwrap();
    let before = t.keys()[1];
    // Existing key: only the value changes, tangents stay.
    assert_eq!(t.set_at(1.0, 5.0, Tan::Linear), Ok(1));
    let k = t.keys()[1];
    assert_eq!(
        (k.v(), k.inn, k.out, k.broken),
        (5.0, before.inn, before.out, before.broken)
    );
    // New time: inserted with the given tangent.
    assert_eq!(t.set_at(1.5, 2.0, Tan::Linear), Ok(2));
    assert_eq!(t.keys()[2].out.kind, Tan::Linear);
    assert_eq!(
        t.set_at(f64::NAN, 0.0, Tan::Linear),
        Err(KeysError::NotFinite)
    );
}

#[test]
fn shift_selection() {
    let mut t = tr(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 4.0), (3.0, 9.0)],
        Tan::Smooth,
    );
    // Duplicated indices move once; order is kept.
    t.shift(&[1, 2, 2], 0.25).unwrap();
    let times = |t: &Track| t.keys().iter().map(Key::t).collect::<Vec<_>>();
    assert_eq!(times(&t), [0.0, 1.25, 2.25, 3.0]);
    let before = t.clone();
    // Landing on an unselected key: error, unchanged. Passing over one is fine and reorders.
    assert_eq!(t.shift(&[1], 1.75), Err(KeysError::Order(3)));
    assert_eq!(t.shift(&[9], 1.0), Err(KeysError::Index));
    assert_eq!(t, before);
    t.shift(&[0], 1.5).unwrap();
    let vals = t.keys().iter().map(Key::v).collect::<Vec<_>>();
    assert_eq!(vals, [1.0, 0.0, 4.0, 9.0]);
    assert_eq!(t, Track::from_keys(t.keys().to_vec()).unwrap());
    t.shift(&[], 5.0).unwrap();
}

/// Midpoint-rule reference integral of `eval` over `[a, b]`, split at every point where the track can
/// have a kink or a jump (key times, repeated by the extrapolation periods, mirrored for ping-pong).
fn ref_integral(t: &Track, a: f64, b: f64) -> f64 {
    let (a, b, sign) = if a <= b { (a, b, 1.0) } else { (b, a, -1.0) };
    let (t0, tn) = t.range().unwrap();
    let span = tn - t0;
    let mut cuts = vec![a, b, t0, tn];
    for m in -40i32..=40 {
        for k in t.keys() {
            let d = k.t() - t0;
            cuts.push(t0 + f64::from(m) * span + d);
            cuts.push(t0 + f64::from(m) * span + (span - d));
        }
    }
    cuts.retain(|x| (a..=b).contains(x));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut total = 0.0;
    for w in cuts.windows(2) {
        let n = 4000;
        let h = (w[1] - w[0]) / f64::from(n);
        total += (0..n)
            .map(|i| t.eval(w[0] + (f64::from(i) + 0.5) * h))
            .sum::<f64>()
            * h;
    }
    sign * total
}

#[test]
fn integrate_matches_numeric_reference() {
    let mut seed = 21;
    let tans = [
        Tan::Smooth,
        Tan::Linear,
        Tan::Flat,
        Tan::CatmullRom,
        Tan::Constant,
    ];
    for round in 0..60 {
        let mut t = random_track(&mut seed, 5, &tans);
        // Weighted handles, so the solve path is integrated too.
        if round % 2 == 0 {
            let (m1, m2) = (lcg(&mut seed) * 3.0 - 1.5, lcg(&mut seed) * 3.0 - 1.5);
            t.set_user(1, SideId::Out, m1, Some(0.3)).unwrap();
            t.set_user(2, SideId::In, m2, Some(0.2 + lcg(&mut seed)))
                .unwrap();
        }
        let (pre, post) = (Extrap::ALL[round % 5], Extrap::ALL[(round / 5) % 5]);
        t.set_extrap(pre, post);
        let (t0, tn) = t.range().unwrap();
        let span = tn - t0;
        for (a, b) in [
            (t0 - 0.37 * span, tn + 0.9 * span),
            (t0 + 0.2, tn - 0.1),
            (tn + 2.3 * span, t0 - 1.7 * span),
            (t0 - 3.1 * span, t0 - 0.4),
            (tn + 0.3, tn + 4.6 * span),
            (t0 + 1.0, t0 + 1.0),
        ] {
            let (got, want) = (t.integrate(a, b), ref_integral(&t, a, b));
            assert!(
                (got - want).abs() < 1e-5 * (1.0 + want.abs()),
                "round {round} {pre:?}/{post:?} [{a},{b}]: {got} vs {want}"
            );
        }
        // Additivity and antisymmetry.
        let (a, m, b) = (t0 - span, t0 + 0.4 * span, tn + span);
        assert!((t.integrate(a, b) - t.integrate(a, m) - t.integrate(m, b)).abs() < 1e-9);
        assert!((t.integrate(a, b) + t.integrate(b, a)).abs() < 1e-12);
    }
    let line = tr(&[(0.0, 1.0), (1.0, 2.0)], Tan::Linear);
    assert!(line.integrate(0.0, f64::NAN).is_nan());
}

#[test]
fn integrate_holds_and_degenerate_tracks_in_closed_form() {
    let mut t = tr(&[(0.0, 1.0), (1.0, 5.0), (2.0, 2.0)], Tan::Linear);
    t.set_side(0, SideId::Out, Side::new(Tan::Constant))
        .unwrap();
    // hold 1 over [0,1], line 5 -> 2 over [1,2]; constant extrapolation 1 before and 2 after.
    assert!((t.integrate(0.0, 2.0) - 4.5).abs() < 1e-12);
    assert!((t.integrate(-1.0, 3.0) - 7.5).abs() < 1e-12);
    // The reversed step (in Constant on the right key) is the right value over the whole open segment.
    let mut r = tr(&[(0.0, 1.0), (2.0, 5.0)], Tan::Linear);
    r.set_side(1, SideId::In, Side::new(Tan::Constant)).unwrap();
    assert!((r.integrate(0.0, 2.0) - 10.0).abs() < 1e-12);
    assert_eq!(Track::<f64>::new().integrate(-3.0, 4.0), 0.0);
    let one = tr(&[(1.0, 2.0)], Tan::Smooth);
    assert!((one.integrate(0.0, 4.0) - 8.0).abs() < 1e-12);
    // Linear extrapolation is a trapezoid: value 2 and slope 1 after t = 2.
    let mut l = tr(&[(0.0, 0.0), (2.0, 2.0)], Tan::Linear);
    l.set_extrap(Extrap::Linear, Extrap::Linear);
    assert!((l.integrate(2.0, 4.0) - (2.0 * 2.0 + 0.5 * 4.0)).abs() < 1e-12);
}

#[test]
fn slope_at_matches_finite_differences() {
    let mut seed = 31;
    let tans = [Tan::Smooth, Tan::Linear, Tan::CatmullRom];
    for round in 0..40 {
        let mut t = random_track(&mut seed, 5, &tans);
        let m = lcg(&mut seed) * 2.0 - 1.0;
        t.set_user(1, SideId::Out, m, Some(0.3)).unwrap();
        t.set_extrap(Extrap::ALL[round % 5], Extrap::ALL[(round / 2) % 5]);
        let (t0, tn) = t.range().unwrap();
        let span = tn - t0;
        let h = 1e-6;
        for s in 0..60 {
            let x = t0 - span + span * 3.0 * (f64::from(s) + 0.37) / 60.0;
            // Skip the neighbourhood of kinks (keys and period joins, mirrored or not).
            let rel = (x - t0).rem_euclid(span);
            let near_kink = t.keys().iter().any(|k| {
                let d = k.t() - t0;
                (rel - d).abs() < 1e-3 || (rel - (span - d)).abs() < 1e-3
            });
            if near_kink || rel < 1e-3 || span - rel < 1e-3 {
                continue;
            }
            let fd = (t.eval(x + h) - t.eval(x - h)) / (2.0 * h);
            let sl = t.slope_at(x);
            assert!(
                (fd - sl).abs() < 1e-4 * (1.0 + sl.abs()),
                "round {round} x={x}: {sl} vs {fd}"
            );
        }
    }
    let mut c = tr(&[(0.0, 0.0), (1.0, 1.0)], Tan::Linear);
    c.set_side(0, SideId::Out, Side::new(Tan::Constant))
        .unwrap();
    assert_eq!(
        (
            c.slope_at(0.5),
            c.slope_at(f64::NAN),
            c.slope_at(f64::INFINITY)
        ),
        (0.0, 0.0, 0.0)
    );
}

#[test]
fn set_seg_toggles_back_to_unbroken() {
    let mut t = tr(
        &[(0.0, 0.0), (1.0, 1.0), (2.0, 3.0), (3.0, 3.0)],
        Tan::Smooth,
    );
    let before = t.clone();
    t.set_seg(1, Tan::Linear).unwrap();
    assert!(t.keys()[1].broken && t.keys()[2].broken);
    t.set_seg(1, Tan::Smooth).unwrap();
    assert!(!t.keys()[1].broken && !t.keys()[2].broken);
    assert_eq!(t, before);
}

#[test]
fn unbroken_invariant_is_enforced_on_entry() {
    let mut k = Key::new(0.0, 0.0);
    k.inn = Side::user(1.0, None);
    k.out = Side::user(2.0, None);
    // broken = false: the in side follows the out side.
    let t = Track::from_keys(vec![k, Key::new(1.0, 1.0)]).unwrap();
    assert_eq!(t.keys()[0].inn.slope, 2.0);
    let mut t = Track::new();
    t.insert(k).unwrap();
    assert!(t.keys()[0].coupled());
    k.broken = true;
    let mut t = Track::new();
    t.insert(k).unwrap();
    assert_eq!(t.keys()[0].inn.slope, 1.0);
}

#[test]
fn freeze_and_set_handle() {
    let mut t = tr(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)], Tan::Smooth);
    let before: Vec<f64> = (0..=20).map(|s| t.eval(f64::from(s) * 0.1)).collect();
    // Freezing keeps the curve and makes the handle explicit (also on the coupled in side).
    t.freeze(0, SideId::Out).unwrap();
    t.freeze(2, SideId::In).unwrap();
    assert_eq!(t.keys()[0].out.kind, Tan::User);
    for (s, b) in before.iter().enumerate() {
        assert!((t.eval(s as f64 * 0.1) - b).abs() < 1e-12);
    }
    // Drag the out handle of key 1 to (0.5, 0.25): slope 0.5, weight 0.5.
    t.set_broken(1, true).unwrap();
    t.set_handle(1, SideId::Out, (0.5, 0.25)).unwrap();
    let o = t.keys()[1].out;
    assert_eq!((o.kind, o.slope, o.weight), (Tan::User, 0.5, Some(0.5)));
    assert_eq!(t.handle(1, SideId::Out), Some((0.5, 0.25)));
    // The in handle at (-0.4, -0.2) has the same slope.
    t.set_handle(1, SideId::In, (-0.4, -0.2)).unwrap();
    let i = t.keys()[1].inn;
    assert_eq!((i.slope, i.weight), (0.5, Some(0.4)));
    // Dragged across the key: weight 0, slope kept.
    t.set_handle(1, SideId::Out, (-0.2, 5.0)).unwrap();
    let o = t.keys()[1].out;
    assert_eq!((o.slope, o.weight), (0.5, Some(0.0)));
    assert_eq!(t.freeze(9, SideId::In), Err(KeysError::Index));
}

#[test]
fn labels_and_lists() {
    assert_eq!(Tan::ALL.len(), 6);
    assert_eq!(Extrap::ALL.len(), 5);
    assert!(Tan::ALL.iter().all(|t| !t.label().is_empty()));
    assert!(Extrap::ALL.iter().all(|e| !e.label().is_empty()));
    assert_eq!(
        Channels::<f64, 2>::try_from(vec![Track::new()]).unwrap_err(),
        KeysError::Count
    );
}

// ---- derived-value overflow: finite inputs must never produce inf ----

const HUGE: f64 = 1e308;

#[test]
fn overflowing_handles_are_rejected_and_roll_back() {
    let mut t = tr(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)], Tan::Smooth);
    let before = t.clone();
    let nf = Err(KeysError::NotFinite);
    // slope * width overflows y1.
    assert_eq!(t.set_user(1, SideId::Out, HUGE, Some(1e10)), nf);
    assert_eq!(t.set_user(1, SideId::In, -HUGE, Some(1e10)), nf);
    assert_eq!(t, before);
    // set_handle: a tiny dt makes dv/dt overflow (rejected by the side check) or slope*weight overflow.
    assert_eq!(t.set_handle(1, SideId::Out, (1e-300, 1e10)), nf);
    assert_eq!(t.set_handle(1, SideId::Out, (0.9, 1e308)), nf);
    // A value near MAX overflows the derived secants.
    assert_eq!(t.set_value(1, f64::MAX), nf);
    assert_eq!(t, before);
    // insert (new key and replace) and add.
    assert_eq!(
        t.add(0.5, f64::MAX, Tan::Smooth).unwrap_err(),
        KeysError::NotFinite
    );
    assert_eq!(
        t.add(1.0, f64::MAX, Tan::Linear).unwrap_err(),
        KeysError::NotFinite
    );
    let mut k = Key::new(0.25, 0.0);
    k.out = Side::user(HUGE, Some(1e10));
    assert_eq!(t.insert(k).unwrap_err(), KeysError::NotFinite);
    assert_eq!(t, before);
    // Moves that squeeze two keys so close that the secant overflows.
    let mut u = tr(&[(0.0, 0.0), (1.0, 1e300), (2.0, 0.0)], Tan::Linear);
    let before_u = u.clone();
    assert_eq!(
        u.move_key(0, 1.0 - 1e-10).unwrap_err(),
        KeysError::NotFinite
    );
    assert_eq!(u.shift(&[0], 1.0 - 1e-10), nf);
    assert_eq!(u, before_u);
    // set_seg / set_tan / set_broken roll back too.
    let mut w = tr(&[(0.0, -1e306), (1e-10, 1e306), (1.0, 0.0)], Tan::Flat);
    let before_w = w.clone();
    assert_eq!(w.set_seg(0, Tan::Linear), nf);
    assert_eq!(w.set_tan(0, Tan::Linear), nf);
    assert_eq!(w, before_w);
    assert_eq!(w.set_broken(0, true), Ok(()));
    // scale_time: weights grow past the span only after clamping, slopes shrink; times overflow first.
    let mut s = tr(&[(0.0, 0.0), (1e300, 1.0)], Tan::Linear);
    let before_s = s.clone();
    assert_eq!(s.scale_time(1e10), nf);
    assert_eq!(s, before_s);
    // Whatever survives evaluates finite everywhere, extrapolation included.
    let mut e = tr(&[(0.0, 1e300), (1.0, -1e300)], Tan::Linear);
    e.set_extrap(Extrap::Linear, Extrap::CycleOffset);
    for x in [-1e308, -1e5, 0.5, 1e5, 1e308] {
        assert!(e.eval(x).is_finite(), "{x}");
    }
}

#[test]
fn from_keys_and_serde_reject_overflowing_handles() {
    let mut a = Key::new(0.0, 0.0);
    a.out = Side::user(HUGE, Some(1e10));
    assert_eq!(
        Track::from_keys(vec![a, Key::new(1e10, 1.0)]).unwrap_err(),
        KeysError::NotFinite
    );
    assert_eq!(
        Track::from_keys(vec![Key::new(0.0, f64::MAX), Key::new(1.0, -f64::MAX)]).unwrap_err(),
        KeysError::NotFinite
    );
    #[cfg(feature = "serde")]
    assert!(
        serde_json::from_str::<Track>(
            r#"{"keys":[{"t":0,"v":0,"out":{"kind":"user","slope":1e307,"weight":1e10}},{"t":1e10,"v":1}]}"#
        )
        .is_err()
    );
}
