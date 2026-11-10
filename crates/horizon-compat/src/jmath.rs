//! `java.lang.Math` as HotSpot computes it on the machine the game runs on: what a mod's own
//! arithmetic, and the libraries' taken from the player's install (JOML), get from `Math.sin` and
//! its kin, to the last bit.
//!
//! Rust's float functions call the platform's libm, and those round differently: glibc, Apple's
//! libm and fdlibm disagree in the last bit for a few percent of arguments. HotSpot's own results
//! differ between machines too, because it replaces some `Math` methods with intrinsics:
//!
//! - `atan2`, `atan`, `asin`, `acos` and `hypot` are `StrictMath`'s (fdlibm) everywhere.
//! - On x86-64, `sin`, `cos`, `tan`, `exp`, `log`, `log10` and `pow` are Intel's libm stubs, which
//!   round like glibc for nearly every argument; there they use the platform libm.
//! - On AArch64 only `sin` and `cos` are intrinsics: fdlibm's kernels evaluated with fused
//!   multiply-adds (`macroAssembler_aarch64_trig.cpp`). Everything else is `StrictMath`.
//!
//! This is Horizon's own `horizon_core::jmath` (and the `atan2` of its `geometry::trig`), carried
//! here because a bundle's crates stand on their own: keep the two alike. The values that pin it
//! to HotSpot are tested there; a few of them here.

/// `Math.sin`.
pub fn sin(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        aarch64::sin(x)
    } else {
        x.sin()
    }
}

/// `Math.cos`.
pub fn cos(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        aarch64::cos(x)
    } else {
        x.cos()
    }
}

/// `Math.tan`.
pub fn tan(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        libm::tan(x)
    } else {
        x.tan()
    }
}

/// `Math.exp`.
pub fn exp(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        libm::exp(x)
    } else {
        x.exp()
    }
}

/// `Math.log`.
pub fn log(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        fdlibm::log(x)
    } else {
        x.ln()
    }
}

/// `Math.log10`.
pub fn log10(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        fdlibm::log10(x)
    } else {
        x.log10()
    }
}

/// `Math.pow`.
pub fn pow(x: f64, y: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        libm::pow(x, y)
    } else {
        x.powf(y)
    }
}

/// `Math.cbrt`.
pub fn cbrt(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        fdlibm::cbrt(x)
    } else {
        x.cbrt()
    }
}

/// `Math.atan2`.
pub fn atan2(y: f64, x: f64) -> f64 {
    fdlibm_atan2(y, x)
}

/// `Math.atan`.
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}

/// `Math.asin`.
pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}

/// `Math.acos`.
pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}

/// `Math.hypot`.
pub fn hypot(x: f64, y: f64) -> f64 {
    fdlibm::hypot(x, y)
}

fn hi(x: f64) -> i32 {
    (x.to_bits() >> 32) as i32
}

fn with_hi(x: f64, high: i32) -> f64 {
    f64::from_bits((x.to_bits() & 0xffff_ffff) | (u64::from(high as u32) << 32))
}

/// `java.lang.FdLibm` ports (`StrictMath`) of what the `libm` crate computes
/// differently.
// fdlibm's constants and NaN, verbatim.
#[allow(clippy::excessive_precision, clippy::approx_constant, clippy::eq_op)]
pub mod fdlibm {
    use super::{hi, with_hi};

    const TWO54: f64 = 1.80143985094819840000e+16;

    /// `FdLibm.Log`.
    pub fn log(mut x: f64) -> f64 {
        const LN2_HI: f64 = 6.93147180369123816490e-01;
        const LN2_LO: f64 = 1.90821492927058770002e-10;
        const LG1: f64 = 6.666666666666735130e-01;
        const LG2: f64 = 3.999999999940941908e-01;
        const LG3: f64 = 2.857142874366239149e-01;
        const LG4: f64 = 2.222219843214978396e-01;
        const LG5: f64 = 1.818357216161805012e-01;
        const LG6: f64 = 1.531383769920937332e-01;
        const LG7: f64 = 1.479819860511658591e-01;
        let mut hx = hi(x);
        let lx = x.to_bits() as u32;
        let mut k = 0;
        if hx < 0x0010_0000 {
            if ((hx & 0x7fff_ffff) as u32 | lx) == 0 {
                return -TWO54 / 0.0;
            }
            if hx < 0 {
                return (x - x) / 0.0;
            }
            k -= 54;
            x *= TWO54;
            hx = hi(x);
        }
        if hx >= 0x7ff0_0000 {
            return x + x;
        }
        k += (hx >> 20) - 1023;
        hx &= 0x000f_ffff;
        let i = (hx + 0x9_5f64) & 0x10_0000;
        x = with_hi(x, hx | (i ^ 0x3ff0_0000));
        k += i >> 20;
        let f = x - 1.0;
        if (0x000f_ffff & (2 + hx)) < 3 {
            if f == 0.0 {
                if k == 0 {
                    return 0.0;
                }
                let dk = f64::from(k);
                return dk * LN2_HI + dk * LN2_LO;
            }
            let r = f * f * (0.5 - 0.33333333333333333 * f);
            if k == 0 {
                return f - r;
            }
            let dk = f64::from(k);
            return dk * LN2_HI - ((r - dk * LN2_LO) - f);
        }
        let s = f / (2.0 + f);
        let dk = f64::from(k);
        let z = s * s;
        let mut i = hx - 0x6_147a;
        let w = z * z;
        let j = 0x6b851 - hx;
        let t1 = w * (LG2 + w * (LG4 + w * LG6));
        let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
        i |= j;
        let r = t2 + t1;
        if i > 0 {
            let hfsq = 0.5 * f * f;
            if k == 0 {
                f - (hfsq - s * (hfsq + r))
            } else {
                dk * LN2_HI - ((hfsq - (s * (hfsq + r) + dk * LN2_LO)) - f)
            }
        } else if k == 0 {
            f - s * (f - r)
        } else {
            dk * LN2_HI - ((s * (f - r) - dk * LN2_LO) - f)
        }
    }

    /// `FdLibm.Log10`.
    pub fn log10(mut x: f64) -> f64 {
        const IVLN10: f64 = 4.34294481903251816668e-01;
        const LOG10_2HI: f64 = 3.01029995663611771306e-01;
        const LOG10_2LO: f64 = 3.69423907715893078616e-13;
        let mut hx = hi(x);
        let lx = x.to_bits() as u32;
        let mut k = 0;
        if hx < 0x0010_0000 {
            if ((hx & 0x7fff_ffff) as u32 | lx) == 0 {
                return -TWO54 / 0.0;
            }
            if hx < 0 {
                return (x - x) / 0.0;
            }
            k -= 54;
            x *= TWO54;
            hx = hi(x);
        }
        if hx >= 0x7ff0_0000 {
            return x + x;
        }
        k += (hx >> 20) - 1023;
        let i = ((k as u32) >> 31) as i32;
        hx = (hx & 0x000f_ffff) | ((0x3ff - i) << 20);
        let y = f64::from(k + i);
        x = with_hi(x, hx);
        let z = y * LOG10_2LO + IVLN10 * log(x);
        z + y * LOG10_2HI
    }

    /// `FdLibm.Cbrt`.
    pub fn cbrt(mut x: f64) -> f64 {
        const B1: i32 = 715094163;
        const B2: i32 = 696219795;
        const C: f64 = 5.42857142857142815906e-01;
        const D: f64 = -7.05306122448979611050e-01;
        const E: f64 = 1.41428571428571436819e+00;
        const F: f64 = 1.60714285714285720630e+00;
        const G: f64 = 3.57142857142857150787e-01;
        if x == 0.0 || !x.is_finite() {
            return x;
        }
        let sign = if x < 0.0 { -1.0 } else { 1.0 };
        x = x.abs();
        let mut t = 0.0f64;
        if x < f64::MIN_POSITIVE {
            t = 18014398509481984.0; // 2^54
            t *= x;
            t = with_hi(t, hi(t) / 3 + B2);
        } else {
            t = with_hi(t, hi(x) / 3 + B1);
        }
        let mut r = t * t / x;
        let mut s = C + r * t;
        t *= G + F / (s + E + D / s);
        t = f64::from_bits(t.to_bits() & 0xffff_ffff_0000_0000);
        t = with_hi(t, hi(t) + 1);
        s = t * t;
        r = x / s;
        let w = t + t;
        r = (r - t) / (w + r);
        t += t * r;
        sign * t
    }

    /// `FdLibm.Hypot`.
    pub fn hypot(x: f64, y: f64) -> f64 {
        let mut a = x.abs();
        let mut b = y.abs();
        if !a.is_finite() || !b.is_finite() {
            if a == f64::INFINITY || b == f64::INFINITY {
                return f64::INFINITY;
            }
            return a + b;
        }
        if b > a {
            std::mem::swap(&mut a, &mut b);
        }
        let mut ha = hi(a);
        let mut hb = hi(b);
        if ha - hb > 0x3c0_0000 {
            return a + b;
        }
        let mut k = 0;
        if a > f64::from_bits(0x5f30_0000_ffff_ffff) {
            ha -= 0x2580_0000;
            hb -= 0x2580_0000;
            a *= f64::from_bits(0x1a70_0000_0000_0000); // 2^-600
            b *= f64::from_bits(0x1a70_0000_0000_0000);
            k += 600;
        }
        if b < f64::from_bits(0x20b0_0000_0000_0000) {
            // 2^-500
            if b < f64::MIN_POSITIVE {
                if b == 0.0 {
                    return a;
                }
                let t1 = f64::from_bits(0x7fd0_0000_0000_0000); // 2^1022
                b *= t1;
                a *= t1;
                k -= 1022;
            } else {
                ha += 0x2580_0000;
                hb += 0x2580_0000;
                a *= f64::from_bits(0x6570_0000_0000_0000); // 2^600
                b *= f64::from_bits(0x6570_0000_0000_0000);
                k -= 600;
            }
        }
        let mut w = a - b;
        if w > b {
            let t1 = with_hi(0.0, ha);
            let t2 = a - t1;
            w = (t1 * t1 - (b * (-b) - t2 * (a + t1))).sqrt();
        } else {
            a += a;
            let y1 = with_hi(0.0, hb);
            let y2 = b - y1;
            let t1 = with_hi(0.0, ha + 0x0010_0000);
            let t2 = a - t1;
            w = (t1 * y1 - (w * (-w) - (t1 * y2 + t2 * b))).sqrt();
        }
        if k != 0 {
            f64::from_bits(((1023 + k) as u64) << 52) * w
        } else {
            w
        }
    }
}

/// HotSpot's AArch64 `Math.sin` / `Math.cos` stubs
/// (`MacroAssembler::generate_dsin_dcos`): fdlibm with fused multiply-adds.
/// Exposed on every architecture so they can be tested anywhere.
// HotSpot's tables, verbatim.
#[allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    clippy::unreadable_literal
)]
pub mod aarch64 {
    use super::hi;

    const S: [f64; 6] = [
        -1.66666666666666324348e-01,
        8.33333333332248946124e-03,
        -1.98412698298579493134e-04,
        2.75573137070700676789e-06,
        -2.50507602534068634195e-08,
        1.58969099521155010221e-10,
    ];
    const C: [f64; 6] = [
        4.16666666666666019037e-02,
        -1.38888888888741095749e-03,
        2.48015872894767294178e-05,
        -2.75573143513906633035e-07,
        2.08757232129817482790e-09,
        -1.13596475577881948265e-11,
    ];
    const INVPIO2: f64 = 6.36619772367581382433e-01;
    const PIO2_1: f64 = 1.57079632673412561417e+00;
    const PIO2_1T: f64 = 6.07710050650619224932e-11;
    const PIO2_2: f64 = 6.07710050630396597660e-11;
    const PIO2_2T: f64 = 2.02226624879595063154e-21;
    const PIO2_3: f64 = 2.02226624871116645580e-21;
    const PIO2_3T: f64 = 8.47842766036889956997e-32;
    const NPIO2_HW: [i32; 32] = [
        0x3FF921FB, 0x400921FB, 0x4012D97C, 0x401921FB, 0x401F6A7A, 0x4022D97C, 0x4025FDBB,
        0x402921FB, 0x402C463A, 0x402F6A7A, 0x4031475C, 0x4032D97C, 0x40346B9C, 0x4035FDBB,
        0x40378FDB, 0x403921FB, 0x403AB41B, 0x403C463A, 0x403DD85A, 0x403F6A7A, 0x40407E4C,
        0x4041475C, 0x4042106C, 0x4042D97C, 0x4043A28C, 0x40446B9C, 0x404534AC, 0x4045FDBB,
        0x4046C6CB, 0x40478FDB, 0x404858EB, 0x404921FB,
    ];

    /// `generate_kernel_sin`.
    fn kernel_sin(x: f64, y: f64, iy: bool) -> f64 {
        let z = x * x;
        let v = z * x;
        let mut r = z.mul_add(S[5], S[4]);
        r = z.mul_add(r, S[3]);
        r = z.mul_add(r, S[2]);
        r = z.mul_add(r, S[1]);
        if !iy {
            v.mul_add(z.mul_add(r, S[0]), x)
        } else {
            let t = (-v).mul_add(r, 0.5 * y);
            let t = (-z).mul_add(t, y);
            x + v.mul_add(S[0], t)
        }
    }

    /// `generate_kernel_cos`. `ix` is the high word of the *original*
    /// argument: the stub reuses the register, so a reduced argument always
    /// takes the `qx = 0.28125` branch.
    fn kernel_cos(x: f64, y: f64, ix: i32) -> f64 {
        let z = x * x;
        let mut r = z.mul_add(C[5], C[4]);
        r = z.mul_add(r, C[3]);
        let xy = x * y;
        r = z.mul_add(r, C[2]);
        r = z.mul_add(r, C[1]);
        let zz = z * z;
        r = z.mul_add(r, C[0]);
        if ix <= 0x3FD33333 {
            let v = (-zz).mul_add(r, xy);
            1.0 - 0.5f64.mul_add(z, v)
        } else {
            let qx = if ix > 0x3fe90000 {
                0.28125
            } else {
                f64::from_bits(u64::from((ix - 0x0020_0000) as u32) << 32)
            };
            let zr_xy = zz.mul_add(r, -xy);
            let h = 0.5f64.mul_add(z, -qx);
            let a = 1.0 - qx;
            a - (h - zr_xy)
        }
    }

    /// `generate__ieee754_rem_pio2` for |x| <= 2^19·π/2; `None` beyond.
    fn rem_pio2(x: f64, ix: i32) -> Option<(i32, f64, f64)> {
        if ix < 0x4002d97c {
            return Some(if x.to_bits() as i64 >= 0 {
                let z = x - PIO2_1;
                if ix != 0x3ff921fb {
                    let y0 = z - PIO2_1T;
                    (1, y0, (z - y0) - PIO2_1T)
                } else {
                    let z = z - PIO2_2;
                    let y0 = z - PIO2_2T;
                    (1, y0, (z - y0) - PIO2_2T)
                }
            } else {
                let z = x + PIO2_1;
                if ix != 0x3ff921fb {
                    let y0 = z + PIO2_1T;
                    (-1, y0, (z - y0) + PIO2_1T)
                } else {
                    let z = z + PIO2_2;
                    let y0 = z + PIO2_2T;
                    (-1, y0, (z - y0) + PIO2_2T)
                }
            });
        }
        if ix > 0x413921fb {
            return None;
        }
        let t = x.abs();
        let fnv = t.mul_add(INVPIO2, 0.5).trunc();
        let n = fnv as i32;
        let mut r = (-fnv).mul_add(PIO2_1, t);
        let mut w = fnv * PIO2_1T;
        let mut y0 = r - w;
        if n > 32 || ix == NPIO2_HW[(n - 1) as usize] {
            let j = ix >> 20;
            let exponent = |y: f64| ((y.to_bits() << 1) >> 53) as i32;
            if j - exponent(y0) > 16 {
                let t = r;
                let w2 = fnv * PIO2_2;
                r = t - w2;
                w = fnv.mul_add(PIO2_2T, w2 - (t - r));
                y0 = r - w;
                if j - exponent(y0) > 49 {
                    let t = r;
                    let w3 = fnv * PIO2_3;
                    r = t - w3;
                    w = fnv.mul_add(PIO2_3T, w3 - (t - r));
                    y0 = r - w;
                }
            }
        }
        let y1 = (r - y0) - w;
        Some(if x > 0.0 { (n, y0, y1) } else { (-n, -y0, -y1) })
    }

    fn sin_cos(x: f64, is_cos: bool) -> f64 {
        let ix = hi(x) & 0x7fff_ffff;
        if ix < 0x3e400000 {
            return if is_cos { 1.0 } else { x };
        }
        if ix <= 0x3fe921fb {
            return if is_cos {
                kernel_cos(x, 0.0, ix)
            } else {
                kernel_sin(x, 0.0, false)
            };
        }
        if ix >= 0x7ff00000 {
            return f64::from_bits(0x7ff0_0000_0000_0001);
        }
        // Beyond 2^19·π/2 the stub's Payne–Hanek reduction isn't ported:
        // StrictMath's is used instead.
        let Some((n, y0, y1)) = rem_pio2(x, ix) else {
            return if is_cos { libm::cos(x) } else { libm::sin(x) };
        };
        let (value, negate) = if (n & 1 == 1) != is_cos {
            (
                kernel_cos(y0, y1, ix),
                if is_cos { (n ^ (n >> 1)) & 1 } else { n & 2 },
            )
        } else {
            (
                kernel_sin(y0, y1, true),
                if is_cos { (n ^ (n >> 1)) & 1 } else { n & 2 },
            )
        };
        if negate != 0 { -value } else { value }
    }

    /// `Math.sin` on AArch64 HotSpot.
    pub fn sin(x: f64) -> f64 {
        sin_cos(x, false)
    }

    /// `Math.cos` on AArch64 HotSpot.
    pub fn cos(x: f64) -> f64 {
        sin_cos(x, true)
    }
}

/// fdlibm `atan`, as `StrictMath.atan` computes it.
#[allow(clippy::excessive_precision, clippy::approx_constant)] // fdlibm's constants, verbatim.
pub fn fdlibm_atan(x: f64) -> f64 {
    const ATANHI: [f64; 4] = [
        4.636_476_090_008_060_935_15e-01,
        7.853_981_633_974_482_789_99e-01,
        9.827_937_232_473_290_540_77e-01,
        1.570_796_326_794_896_558_00e+00,
    ];
    const ATANLO: [f64; 4] = [
        2.269_877_745_296_168_709_24e-17,
        3.061_616_997_868_383_017_93e-17,
        1.390_331_103_123_099_845_16e-17,
        6.123_233_995_736_766_035_87e-17,
    ];
    const AT: [f64; 11] = [
        3.333_333_333_333_293_180_27e-01,
        -1.999_999_999_987_648_324_76e-01,
        1.428_571_427_250_346_637_11e-01,
        -1.111_111_040_546_235_578_80e-01,
        9.090_887_133_436_506_561_96e-02,
        -7.691_876_205_044_829_994_95e-02,
        6.661_073_137_387_531_206_69e-02,
        -5.833_570_133_790_573_486_45e-02,
        4.976_877_994_615_932_360_17e-02,
        -3.653_157_274_421_691_552_70e-02,
        1.628_582_011_536_578_236_23e-02,
    ];
    let hx = (x.to_bits() >> 32) as i32;
    let ix = hx & 0x7fff_ffff;
    let mut x = x;
    let id: i32;
    if ix >= 0x4410_0000 {
        // |x| >= 2^66
        if ix > 0x7ff0_0000 || (ix == 0x7ff0_0000 && (x.to_bits() as u32) != 0) {
            return x + x; // NaN
        }
        return if hx > 0 {
            ATANHI[3] + ATANLO[3]
        } else {
            -ATANHI[3] - ATANLO[3]
        };
    }
    if ix < 0x3fdc_0000 {
        // |x| < 0.4375
        if ix < 0x3e20_0000 && 1.0e300 + x > 1.0 {
            // |x| < 2^-29: raise inexact
            return x;
        }
        id = -1;
    } else {
        x = x.abs();
        if ix < 0x3ff3_0000 {
            if ix < 0x3fe6_0000 {
                id = 0;
                x = (2.0 * x - 1.0) / (2.0 + x);
            } else {
                id = 1;
                x = (x - 1.0) / (x + 1.0);
            }
        } else if ix < 0x4003_8000 {
            id = 2;
            x = (x - 1.5) / (1.0 + 1.5 * x);
        } else {
            id = 3;
            x = -1.0 / x;
        }
    }
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    if id < 0 {
        return x - x * (s1 + s2);
    }
    let i = id as usize;
    let z = ATANHI[i] - ((x * (s1 + s2) - ATANLO[i]) - x);
    if hx < 0 { -z } else { z }
}

/// fdlibm `__ieee754_atan2`, as `StrictMath.atan2` (and so `Math.atan2`)
/// computes it.
#[allow(clippy::excessive_precision, clippy::approx_constant)] // fdlibm's constants, verbatim.
pub fn fdlibm_atan2(y: f64, x: f64) -> f64 {
    const TINY: f64 = 1.0e-300;
    const PI_O_4: f64 = 7.853_981_633_974_482_789_99e-01;
    const PI_O_2: f64 = 1.570_796_326_794_896_558_00e+00;
    const PI: f64 = 3.141_592_653_589_793_115_98e+00;
    const PI_LO: f64 = 1.224_646_799_147_353_207_17e-16;
    let (xb, yb) = (x.to_bits(), y.to_bits());
    let hx = (xb >> 32) as i32;
    let ix = hx & 0x7fff_ffff;
    let lx = xb as u32;
    let hy = (yb >> 32) as i32;
    let iy = hy & 0x7fff_ffff;
    let ly = yb as u32;
    if (ix as u32 | ((lx | lx.wrapping_neg()) >> 31)) > 0x7ff0_0000
        || (iy as u32 | ((ly | ly.wrapping_neg()) >> 31)) > 0x7ff0_0000
    {
        return x + y; // NaN
    }
    if hx.wrapping_sub(0x3ff0_0000) as u32 | lx == 0 {
        return fdlibm_atan(y); // x = 1.0
    }
    let m = ((hy >> 31) & 1) | ((hx >> 30) & 2);
    if (iy as u32 | ly) == 0 {
        return match m {
            0 | 1 => y,
            2 => PI + TINY,
            _ => -PI - TINY,
        };
    }
    if (ix as u32 | lx) == 0 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }
    if ix == 0x7ff0_0000 {
        if iy == 0x7ff0_0000 {
            return match m {
                0 => PI_O_4 + TINY,
                1 => -PI_O_4 - TINY,
                2 => 3.0 * PI_O_4 + TINY,
                _ => -3.0 * PI_O_4 - TINY,
            };
        }
        return match m {
            0 => 0.0,
            1 => -0.0,
            2 => PI + TINY,
            _ => -PI - TINY,
        };
    }
    if iy == 0x7ff0_0000 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }
    let k = (iy - ix) >> 20;
    let z = if k > 60 {
        PI_O_2 + 0.5 * PI_LO
    } else if hx < 0 && k < -60 {
        0.0
    } else {
        fdlibm_atan((y / x).abs())
    };
    match m {
        0 => z,
        1 => -z,
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

#[cfg(test)]
mod tests {
    /// (argument bits, `Math` result bits) from HotSpot on an Apple M4, each one where another
    /// libm rounds differently (`horizon_core::jmath`'s own tests hold the full lists).
    #[test]
    fn matches_hotspot_on_aarch64() {
        let one = |f: fn(f64) -> f64, x: u64| f(f64::from_bits(x)).to_bits();
        assert_eq!(
            one(super::aarch64::sin, 0x3fe63c5ab66b09f0),
            0x3fe47d34215b54f7
        );
        assert_eq!(
            one(super::aarch64::sin, 0x4072c3f15b47b5d6),
            0xbfef32be0f0a9124
        );
        assert_eq!(
            one(super::aarch64::cos, 0x3fbb10690eeadb00),
            0x3fefd24375dbae28
        );
        assert_eq!(
            one(super::aarch64::cos, 0x40099262f89e73f8),
            0xbfeff3aa148795ce
        );
        assert_eq!(
            one(super::fdlibm::log, 0x40569150fe105e6f),
            0x401202e102f27b78
        );
        assert_eq!(
            one(super::fdlibm::log10, 0x3ff5dec70f518390),
            0x3fc15fb52aa5f051
        );
        assert_eq!(
            one(super::fdlibm::cbrt, 0x401bb21f8568bd0c),
            0x3ffe7ee30c7a56f9
        );
        assert_eq!(one(libm::exp, 0x3fdc5999df9270f4), 0x3ff8eacc53fdbf56);
        assert_eq!(one(libm::tan, 0x40023034f56881f0), 0xbff2e3f762ac9810);
        let two = |f: fn(f64, f64) -> f64, x: u64, y: u64| {
            f(f64::from_bits(x), f64::from_bits(y)).to_bits()
        };
        assert_eq!(
            two(super::fdlibm::hypot, 0xc0569150fe105e6f, 0xc0056b970bf3b41a),
            0x405693db72f461a6
        );
        assert_eq!(
            two(libm::pow, 0x3fee04c313607ea6, 0xc00fd156138baf96),
            0x3ff4a199ac2c8e9c
        );
    }
}
