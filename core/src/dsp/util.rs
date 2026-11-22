//! Drobne prymitywy: konwersje dB, wygładzanie parametrów, współczynniki czasowe, opóźnienia, min. przesuwne.

#[inline]
pub fn db_to_lin(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}
#[inline]
pub fn lin_to_db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}
/// Współczynnik filtru jednobiegunowego dla stałej czasowej τ [s]: y += (1−α)(x−y).
#[inline]
pub fn tau_coeff(tau_s: f64, fs: f64) -> f64 {
    if tau_s <= 0.0 {
        0.0
    } else {
        (-1.0 / (tau_s * fs)).exp()
    }
}
#[inline]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
#[inline]
pub fn flush(x: f64) -> f64 {
    if x.abs() < 1e-30 {
        0.0
    } else {
        x
    }
}

/// Iloczyn skalarny z czterema niezależnymi akumulatorami: krótszy łańcuch zależności i autowektoryzacja
/// (sumowanie sekwencyjne f64 nie może być wektoryzowane przez kompilator). Używany w FIR-ach oversamplingu
/// i interpolatorze true-peak – to najgorętsze pętle toru.
#[inline(always)]
pub fn dot(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    let mut acc = [0.0f64; 4];
    let mut ca = a.chunks_exact(4);
    let mut cb = b.chunks_exact(4);
    for (x, y) in (&mut ca).zip(&mut cb) {
        acc[0] += x[0] * y[0];
        acc[1] += x[1] * y[1];
        acc[2] += x[2] * y[2];
        acc[3] += x[3] * y[3];
    }
    let mut s = (acc[0] + acc[1]) + (acc[2] + acc[3]);
    for (x, y) in ca.remainder().iter().zip(cb.remainder()) {
        s += x * y;
    }
    s
}

/// Wygładzacz parametru (jednobiegunowy, τ domyślnie 20 ms).
#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    pub value: f64,
    target: f64,
    a: f64,
}

impl Smoother {
    pub fn new(v: f64, tau_s: f64, fs: f64) -> Self {
        Self { value: v, target: v, a: tau_coeff(tau_s, fs) }
    }
    pub fn set_target(&mut self, t: f64) {
        self.target = t;
    }
    pub fn target(&self) -> f64 {
        self.target
    }
    pub fn snap(&mut self) {
        self.value = self.target;
    }
    #[inline]
    pub fn tick(&mut self) -> f64 {
        self.value = self.target + self.a * (self.value - self.target);
        if (self.value - self.target).abs() < 1e-9 {
            self.value = self.target;
        }
        self.value
    }
    /// Przesunięcie o n próbek naraz (aktualizacja raz na blok).
    pub fn advance(&mut self, n: usize) -> f64 {
        let k = self.a.powi(n as i32);
        self.value = self.target + k * (self.value - self.target);
        if (self.value - self.target).abs() < 1e-9 {
            self.value = self.target;
        }
        self.value
    }
    pub fn is_settled(&self) -> bool {
        self.value == self.target
    }
}

/// Mono linia opóźniająca o stałej długości.
#[derive(Clone, Debug)]
pub struct Delay {
    buf: Vec<f64>,
    pos: usize,
}

impl Delay {
    pub fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len], pos: 0 }
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }
    #[inline]
    pub fn tick(&mut self, x: f64) -> f64 {
        if self.buf.is_empty() {
            return x;
        }
        let y = self.buf[self.pos];
        self.buf[self.pos] = x;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        y
    }
}

/// Minimum przesuwne z okna długości `len` (kolejka monotoniczna, O(1) zamortyzowane).
#[derive(Clone, Debug)]
pub struct SlidingMin {
    len: usize,
    idx: Vec<u64>,
    val: Vec<f64>,
    head: usize,
    tail: usize,
    count: usize,
    t: u64,
}

impl SlidingMin {
    pub fn new(len: usize) -> Self {
        let cap = len.max(1) + 1;
        Self { len: len.max(1), idx: vec![0; cap], val: vec![0.0; cap], head: 0, tail: 0, count: 0, t: 0 }
    }
    pub fn reset(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.count = 0;
        self.t = 0;
    }
    #[inline]
    pub fn push(&mut self, x: f64) -> f64 {
        let cap = self.idx.len();
        // usuń z końca wartości ≥ x
        while self.count > 0 {
            let last = (self.tail + cap - 1) % cap;
            if self.val[last] >= x {
                self.tail = last;
                self.count -= 1;
            } else {
                break;
            }
        }
        self.idx[self.tail] = self.t;
        self.val[self.tail] = x;
        self.tail = (self.tail + 1) % cap;
        self.count += 1;
        // usuń z początku elementy spoza okna
        while self.count > 0 && self.idx[self.head] + (self.len as u64) <= self.t {
            self.head = (self.head + 1) % cap;
            self.count -= 1;
        }
        self.t += 1;
        self.val[self.head]
    }
}

/// Średnia ruchoma z okna długości `len` (suma bieżąca z okresową rekalkulacją przeciw dryfowi).
#[derive(Clone, Debug)]
pub struct MovingAverage {
    buf: Vec<f64>,
    pos: usize,
    sum: f64,
    since: usize,
}

impl MovingAverage {
    pub fn new(len: usize, init: f64) -> Self {
        let len = len.max(1);
        Self { buf: vec![init; len], pos: 0, sum: init * len as f64, since: 0 }
    }
    pub fn reset(&mut self, init: f64) {
        self.buf.fill(init);
        self.sum = init * self.buf.len() as f64;
        self.pos = 0;
        self.since = 0;
    }
    #[inline]
    pub fn push(&mut self, x: f64) -> f64 {
        self.sum += x - self.buf[self.pos];
        self.buf[self.pos] = x;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        self.since += 1;
        if self.since >= 4096 {
            // spec v1 §3.10: sumę bieżącą przeliczać od nowa co 4096 próbek (dryf zaokrągleń)
            self.sum = self.buf.iter().sum();
            self.since = 0;
        }
        self.sum / self.buf.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sliding_min_matches_naive() {
        let xs: Vec<f64> = (0..500).map(|i| ((i * 7919) % 101) as f64).collect();
        let mut m = SlidingMin::new(17);
        for i in 0..xs.len() {
            let got = m.push(xs[i]);
            let lo = i.saturating_sub(16);
            let want = xs[lo..=i].iter().cloned().fold(f64::INFINITY, f64::min);
            assert_eq!(got, want, "i={i}");
        }
    }
}
