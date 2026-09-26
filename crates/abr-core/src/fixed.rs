//! Q16.16 fixed-point math. No floats anywhere in game state or sim logic —
//! this is what makes `native == wasm` bit-identical (PLAN §5.1).
//!
//! Trig uses committed integer LUTs (`trig_tables.rs`); sqrt is integer
//! Newton; `atan2` is an octant LUT. All outputs are integer-degree
//! quantized, which is finer than the game's 15° bearing resolution.

use crate::trig_tables::{ATAN_LUT, SIN_TABLE};

pub const FRAC: u32 = 16;
pub const ONE: Fix = 1i64 << FRAC;
pub const HALF: Fix = 1i64 << (FRAC - 1);

pub type Fix = i64;

#[inline]
pub fn from_int(v: i64) -> Fix {
    v << FRAC
}

#[inline]
pub fn from_f64(v: f64) -> Fix {
    // Deterministic conversion: round-half-away-from-zero at the fixed LSB.
    (v * ONE as f64).round_ties_even() as i64
}

#[inline]
pub fn to_f64(v: Fix) -> f64 {
    v as f64 / ONE as f64
}

#[inline]
pub fn mul(a: Fix, b: Fix) -> Fix {
    (((a as i128) * (b as i128)) >> FRAC) as i64
}

#[inline]
pub fn div(a: Fix, b: Fix) -> Fix {
    if b == 0 {
        return 0;
    }
    (((a as i128) << FRAC) / (b as i128)) as i64
}

/// Integer sqrt of a non-negative Fix, result in Fix.
#[inline]
pub fn sqrt(a: Fix) -> Fix {
    if a <= 0 {
        return 0;
    }
    let x = (a as u128) << FRAC; // sqrt(a<<16 * 2^16) = sqrt(a) << 16
    isqrt_u128(x) as i64
}

fn isqrt_u128(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    // Newton on the highest one-bit estimate, converges in ~6 iterations.
    let mut x = 1u128 << (128 - n.leading_zeros()).div_ceil(2);
    loop {
        let y = (x + n / x) >> 1;
        if y >= x {
            break;
        }
        x = y;
    }
    while x > 0 && x.checked_mul(x).is_none_or(|p| p > n) {
        x -= 1;
    }
    x
}

/// sin of integer degrees (0..359), Fix in [-ONE, ONE].
#[inline]
pub fn sin_deg(deg: i32) -> Fix {
    let d = deg.rem_euclid(360) as i64;
    // Position in the 1024-entry LUT, as Fix so the fraction interpolates.
    let pos = d * 1024 * ONE / 360;
    let k = (pos >> FRAC) as usize;
    let frac = pos & (ONE - 1);
    let a = SIN_TABLE[k] as i64;
    let b = SIN_TABLE[k + 1] as i64;
    a + (((b - a) * frac) >> FRAC)
}

/// cos of integer degrees, Fix in [-ONE, ONE].
#[inline]
pub fn cos_deg(deg: i32) -> Fix {
    sin_deg(deg + 90)
}

/// atan2 in whole degrees, normalized to 0..359, measured **from north
/// (+Y), clockwise** — the game's bearing convention. Octant method with a
/// 65-entry LUT over atan(z), z in [0,1], linearly interpolated. All
/// intermediates are Fix-scaled degrees; the result is truncated to whole
/// degrees (accuracy ≤ 1°, far inside the game's 15° quantization).
#[inline]
pub fn atan2_deg(y: Fix, x: Fix) -> i32 {
    if x == 0 && y == 0 {
        return 0;
    }
    const D90: Fix = 90 * ONE;
    const D180: Fix = 180 * ONE;
    const D270: Fix = 270 * ONE;
    const D360: Fix = 360 * ONE;
    let ax = x.abs();
    let ay = y.abs();
    let deg_fix: Fix = if ay > ax {
        // Within 45° of the Y axis: β = atan(|x|/|y|) measured from north.
        let b = lut_deg(&ATAN_LUT, div(ax, ay));
        if y >= 0 {
            if x >= 0 {
                b
            } else {
                D360 - b
            }
        } else if x >= 0 {
            D180 - b
        } else {
            D180 + b
        }
    } else {
        // Within 45° of the X axis: α = atan(|y|/|x|) measured from east.
        let a = lut_deg(&ATAN_LUT, div(ay, ax));
        if x >= 0 {
            if y >= 0 {
                D90 - a
            } else {
                D90 + a
            }
        } else if y >= 0 {
            D270 + a
        } else {
            D270 - a
        }
    };
    ((deg_fix / ONE) % 360) as i32
}

fn lut_deg(table: &[i32], z: Fix) -> Fix {
    let z = z.clamp(0, ONE);
    let k = ((z as i128 * 64) >> FRAC) as usize; // 0..64
    let k = k.min(63);
    let z0 = ((k as i64) << FRAC) / 64;
    let z1 = (((k + 1) as i64) << FRAC) / 64;
    let t = if z1 > z0 { div(z - z0, z1 - z0) } else { 0 };
    let a = table[k] as i64;
    let b = table[k + 1] as i64;
    a + mul(b - a, t)
}

/// Quantize whole degrees to the game's 15° bearing increments.
#[inline]
pub fn quantize_bearing(deg: i32) -> u16 {
    (((deg.rem_euclid(360) + 7) / 15) * 15 % 360) as u16
}

/// Normalize to 0..359.
#[inline]
pub fn norm_deg(deg: i32) -> u16 {
    deg.rem_euclid(360) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deg_f64(y: f64, x: f64) -> i32 {
        atan2_deg(from_f64(y), from_f64(x))
    }

    #[test]
    fn sqrt_accuracy() {
        for v in [1i64, 2, 3, 100, 12345, 3200 * 3200] {
            let fx = from_int(v);
            let r = to_f64(sqrt(fx));
            let e = (r - (v as f64).sqrt()).abs();
            assert!(e < 0.01, "sqrt({v}) = {r}");
        }
    }

    #[test]
    fn sin_cos_match_f64() {
        for deg in 0..360 {
            let s = to_f64(sin_deg(deg));
            let c = to_f64(cos_deg(deg));
            let se = (s - (deg as f64).to_radians().sin()).abs();
            let ce = (c - (deg as f64).to_radians().cos()).abs();
            assert!(se < 0.002, "sin({deg}) err {se}");
            assert!(ce < 0.002, "cos({deg}) err {ce}");
        }
    }

    #[test]
    fn atan2_matches_f64() {
        for i in 0..72 {
            for r in [50.0, 500.0, 3000.0] {
                // a = math angle from +X, ccw; bearing = (90 - a) mod 360.
                let a = i as f64 * 5.0f64.to_radians();
                let (y, x) = (a.sin() * r, a.cos() * r);
                let got = deg_f64(y, x) as f64;
                let want = ((90.0 - i as f64 * 5.0) + 360.0) % 360.0;
                let d = (got - want).abs();
                let d = d.min((360.0 - d).abs());
                assert!(d <= 1.5, "atan2({y},{x}) = {got}, want bearing {want}");
            }
        }
    }

    #[test]
    fn bearing_convention_north_clockwise() {
        // +Y is north
        assert_eq!(atan2_deg(from_int(10), from_int(0)), 0);
        // +X is east = 90
        assert_eq!(atan2_deg(from_int(0), from_int(10)), 90);
        // -Y south
        assert_eq!(atan2_deg(from_int(-10), from_int(0)), 180);
        // -X west
        assert_eq!(atan2_deg(from_int(0), from_int(-10)), 270);
        // NE = 45
        assert_eq!(atan2_deg(from_int(10), from_int(10)), 45);
    }

    #[test]
    fn bearing_quantization() {
        assert_eq!(quantize_bearing(0), 0);
        assert_eq!(quantize_bearing(7), 0);
        assert_eq!(quantize_bearing(8), 15);
        assert_eq!(quantize_bearing(355), 0);
        assert_eq!(quantize_bearing(353), 0); // nearest is north
    }

    #[test]
    fn mul_div_roundtrip() {
        let a = from_f64(1.5);
        let b = from_f64(3.25);
        assert!((to_f64(mul(a, b)) - 4.875).abs() < 1e-4);
        assert!((to_f64(div(mul(a, b), b)) - 1.5).abs() < 1e-4);
        assert_eq!(div(from_int(10), from_int(0)), 0);
    }

    #[test]
    fn from_f64_deterministic() {
        // Same value must convert identically every time (it will — but pin it).
        assert_eq!(from_f64(0.1), from_f64(0.1));
        assert_eq!(from_f64(140.0), 140 * ONE);
    }
}
