//! Tiny deterministic noise + RNG helpers (no external crates).

pub fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x85eb_ca6b) ^ (y as u32).wrapping_mul(0xc2b2_ae35) ^ seed.wrapping_mul(0x27d4_eb2f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h
}

fn grad(ix: i32, iy: i32, seed: u32, dx: f32, dy: f32) -> f32 {
    let a = (hash2(ix, iy, seed) & 0xffff) as f32 / 65535.0 * std::f32::consts::TAU;
    a.cos() * dx + a.sin() * dy
}

fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// 2D gradient noise, roughly in [-1, 1].
pub fn gnoise(x: f32, y: f32, seed: u32) -> f32 {
    let x0 = x.floor();
    let y0 = y.floor();
    let (ix, iy) = (x0 as i32, y0 as i32);
    let (fx, fy) = (x - x0, y - y0);
    let (u, v) = (fade(fx), fade(fy));
    let n00 = grad(ix, iy, seed, fx, fy);
    let n10 = grad(ix + 1, iy, seed, fx - 1.0, fy);
    let n01 = grad(ix, iy + 1, seed, fx, fy - 1.0);
    let n11 = grad(ix + 1, iy + 1, seed, fx - 1.0, fy - 1.0);
    let a = n00 + (n10 - n00) * u;
    let b = n01 + (n11 - n01) * u;
    (a + (b - a) * v) * 1.42
}

/// Fractal noise, roughly in [-1, 1].
pub fn fbm(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += gnoise(x * freq, y * freq, seed.wrapping_add(o * 131)) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Small xorshift RNG; good enough for scattering bears.
#[derive(Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut r = Rng(seed ^ 0x9e37_79b9_7f4a_7c15);
        r.next_u32();
        r.next_u32();
        r
    }
    pub fn for_cell(x: i32, y: i32, seed: u32, salt: u32) -> Self {
        Self::new(((hash2(x, y, seed) as u64) << 32) | hash2(y, x, seed ^ salt.wrapping_mul(0x9e37_79b9)) as u64)
    }
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }
    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    pub fn int(&mut self, a: i32, b: i32) -> i32 {
        a + (self.f() * (b - a + 1) as f32) as i32
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }
    pub fn signed(&mut self) -> f32 {
        self.f() * 2.0 - 1.0
    }
}
