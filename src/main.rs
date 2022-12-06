#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
enum Key {
    A,
    ASharp,
    B,
    C,
    CSharp,
    D,
    DSharp,
    E,
    F,
    FSharp,
    G,
    GSharp,
}

impl Key {
    fn note(self, octave: i32) -> Note {
        Note((octave - 4) * 12 + match self {
            Self::C => -9,
            Self::CSharp => -8,
            Self::D => -7,
            Self::DSharp => -6,
            Self::E => -5,
            Self::F => -4,
            Self::FSharp => -3,
            Self::G => -2,
            Self::GSharp => -1,
            Self::A => 0,
            Self::ASharp => 1,
            Self::B => 2,
        })
    }
}

#[derive(Copy, Clone, Debug)]
struct Note(i32);

impl Note {
    // The most common tuning today: 12 tone equal temperament, A440.
    fn hz(self) -> f32 {
        440.0 * 2f32.powf(self.0 as f32 / 12.0)
    }

    fn sine(self) -> Sine<f32> {
        sine(self.hz())
    }
}

fn main() {
    let config = hack::Config::get();

    let mut source =
        Key::A.note(4).sine().vibrato(2.0, 6.0).wrap() *
            adsr(
                0.0..3.1,
                8.0,
                15.0,
                0.6,
                1.0,
            ) +
            Key::C.note(4).sine().vibrato(50.0, 8.0).wrap() *
                adsr(
                    1.0..3.2,
                    8.0,
                    15.0,
                    0.6,
                    1.0,
                ) +
            Key::F.note(4).sine().vibrato(50.0, 14.0).wrap() *
                adsr(
                    2.0..3.4,
                    8.0,
                    15.0,
                    0.6,
                    1.0,
                );

    let (tx, rx) = std::sync::mpsc::channel();

    let channels = config.channels();
    let sample_rate = config.sample_rate();

    let stream = config.create_stream(
        move |buf, info| {
            tx.send(info.timestamp()).unwrap();
            for channels in buf.chunks_mut(channels as usize) {
                source.update(SampleTime {
                    count: 1,
                    rate: sample_rate,
                });
                channels.fill(source.sample());
            }
        }
    );

    stream.play();

    let play_duration = std::time::Duration::from_secs(5);

    let start_time = rx.recv().unwrap().playback;

    for playback_time in rx.iter() {
        if let Some(duration) = playback_time.playback.duration_since(&start_time) {
            // println!("{}", duration.as_millis());
            if duration > play_duration {
                let playback_delay = playback_time.playback.duration_since(&playback_time.callback);
                std::thread::sleep(playback_delay.unwrap());
                break;
            }
        }
    }
}

#[derive(Copy, Clone)]
struct SampleTime {
    pub count: u32,
    // max at 48kHz is about 24 hours
    pub rate: u32,
}

impl SampleTime {
    fn as_secs(&self) -> f32 {
        self.count as f32 / self.rate as f32
    }
}

impl PartialEq for SampleTime {
    fn eq(&self, other: &Self) -> bool {
        // A/B == C/D <=> A*D == C*B
        self.count * other.rate == other.count * self.rate
    }
}

impl PartialOrd for SampleTime {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // A/B < C/D <=> A*D < C*B
        (self.count * other.rate).partial_cmp(&(other.count * self.rate))
    }
}

trait Source {
    type Sample;

    fn wrap(self) -> Wrapped<Self> where Self: Sized {
        Wrapped(self)
    }

    fn update(&mut self, _elapsed: SampleTime) {}

    fn sample(&self) -> Self::Sample;
}

impl Source for f32 {
    type Sample = Self;

    fn sample(&self) -> Self::Sample {
        *self
    }
}

struct Wrapped<T>(T);

impl<T> Wrapped<T> {
    fn unwrap(self) -> T {
        self.0
    }
}

impl<T> Source for Wrapped<T>
    where T: Source
{
    type Sample = T::Sample;

    fn update(&mut self, elapsed: SampleTime) {
        self.0.update(elapsed)
    }

    fn sample(&self) -> Self::Sample {
        self.0.sample()
    }
}

impl<T> Source for Box<T>
    where T: Source
{
    type Sample = T::Sample;

    fn update(&mut self, elapsed: SampleTime) {
        (**self).update(elapsed)
    }

    fn sample(&self) -> Self::Sample {
        (**self).sample()
    }
}

struct Const<T> {
    value: T,
}

impl<T: Clone> Source for Const<T> {
    type Sample = T;

    fn sample(&self) -> T {
        self.value.clone()
    }
}

struct Sine<Hz> {
    hz: Hz,
    phase: f32,
}

impl<Hz> Sine<Hz> {
    fn vibrato<VibHz>(self, hz: VibHz, cents: f32) -> Sine<Add<Hz, Mul<Sine<VibHz>, f32>>> {
        let modulation = Mul { left: sine(hz), right: cents / 14.0 };
        Sine { hz: Add { left: self.hz, right: modulation }, phase: self.phase }
    }
}

impl<Hz> Source for Sine<Hz>
    where Hz: Source<Sample=f32>,
{
    type Sample = f32;

    fn update(&mut self, elapsed: SampleTime) {
        self.hz.update(elapsed);
        self.phase = (self.phase + elapsed.as_secs() * self.hz.sample()).fract();
    }

    fn sample(&self) -> f32 {
        (self.phase * std::f32::consts::TAU).sin()
    }
}

fn sine<Hz>(hz: Hz) -> Sine<Hz> {
    Sine { hz, phase: 0.0 }
}

enum ADSRState {
    Before,
    Attack,
    Decay,
    Sustain,
    Release,
    After,
}

struct ADSR {
    active: std::ops::Range<f32>,
    attack_rate: f32,
    decay_rate: f32,
    sustain_level: f32,
    release_rate: f32,

    time: f32,
    state: ADSRState,
    level: f32,
}

impl Source for ADSR {
    type Sample = f32;

    fn update(&mut self, elapsed: SampleTime) {
        let elapsed = elapsed.as_secs();
        self.time += elapsed;
        match self.state {
            ADSRState::Before => {
                if self.active.contains(&self.time) {
                    self.state = ADSRState::Attack;
                }
            }
            ADSRState::Attack => {
                self.level += self.attack_rate * elapsed;
                if self.level > 1.0 {
                    self.level = 1.0;
                    self.state = ADSRState::Decay;
                }
            }
            ADSRState::Decay => {
                self.level -= self.decay_rate * elapsed;
                if self.level < self.sustain_level {
                    self.level = self.sustain_level;
                    self.state = ADSRState::Sustain;
                }
            }
            ADSRState::Sustain => {
                if !self.active.contains(&self.time) {
                    self.state = ADSRState::Release;
                }
            }
            ADSRState::Release => {
                self.level -= self.release_rate * elapsed;
                if self.level < 0.0 {
                    self.level = 0.0;
                    self.state = ADSRState::After;
                }
            }
            ADSRState::After => {}
        }
    }

    fn sample(&self) -> Self::Sample {
        self.level
    }
}

fn adsr(active: std::ops::Range<f32>, attack_rate: f32, decay_rate: f32, sustain_level: f32, release_rate: f32) -> ADSR {
    ADSR {
        active,
        attack_rate,
        decay_rate,
        sustain_level,
        release_rate,
        time: 0.0,
        state: ADSRState::Before,
        level: 0.0,
    }
}

struct Add<L, R> {
    left: L,
    right: R,
}

impl<L, R> Source for Add<L, R> where L: Source, R: Source, L::Sample: std::ops::Add<R::Sample> {
    type Sample = <L::Sample as std::ops::Add<R::Sample>>::Output;

    fn update(&mut self, elapsed: SampleTime) {
        self.left.update(elapsed);
        self.right.update(elapsed);
    }

    fn sample(&self) -> Self::Sample {
        self.left.sample() + self.right.sample()
    }
}

struct Mul<L, R> {
    left: L,
    right: R,
}

impl<L, R> Source for Mul<L, R>
    where L: Source,
          R: Source,
          L::Sample: std::ops::Mul<R::Sample>
{
    type Sample = <L::Sample as std::ops::Mul<R::Sample>>::Output;

    fn update(&mut self, elapsed: SampleTime) {
        self.left.update(elapsed);
        self.right.update(elapsed);
    }

    fn sample(&self) -> Self::Sample {
        self.left.sample() * self.right.sample()
    }
}

impl<L, R> std::ops::Add<R> for Wrapped<L> {
    type Output = Wrapped<Add<L, R>>;

    fn add(self, rhs: R) -> Self::Output {
        Wrapped(Add { left: self.0, right: rhs })
    }
}

impl<L, R> std::ops::Mul<R> for Wrapped<L> {
    type Output = Wrapped<Mul<L, R>>;

    fn mul(self, rhs: R) -> Self::Output {
        Wrapped(Mul { left: self.0, right: rhs })
    }
}
