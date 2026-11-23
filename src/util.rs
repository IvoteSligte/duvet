use std::sync::mpsc;
use std::time::Duration;

use fps_ticker::Fps;

// Adapted from the gcd crate
pub const fn gcd(a: u32, b: u32) -> u32 {
    // variable names based off euclidean division equation: a = b · q + r
    let (mut a, mut b) = if a > b { (a, b) } else { (b, a) };
    while b != 0 {
        std::mem::swap(&mut a, &mut b);
        b %= a;
    }
    a
}

pub struct SyncQueue<T> {
    timeout: Duration,
    sender: mpsc::SyncSender<T>,
    receiver: mpsc::Receiver<T>,
}

impl<T> SyncQueue<T> {
    pub fn new(capacity: usize, timeout: Duration) -> (Self, Self) {
        // Create bidirectional channel as two unidirectional channels
        let (a_sender, b_receiver) = mpsc::sync_channel(capacity);
        let (b_sender, a_receiver) = mpsc::sync_channel(capacity);
        let a = Self {
            timeout,
            sender: a_sender,
            receiver: a_receiver,
        };
        let b = Self {
            timeout,
            sender: b_sender,
            receiver: b_receiver,
        };
        (a, b)
    }

    #[inline(always)]
    pub fn push(&mut self, value: T) -> Result<(), mpsc::SendError<T>> {
        self.sender.send(value)
    }

    #[inline(always)]
    pub fn pop(&mut self) -> Result<T, mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(self.timeout)
    }
}

pub struct FrameRateLimiter {
    target_frame_rate: u32,
    sleep_duration: Duration,
    fps: Fps,
}

impl FrameRateLimiter {
    pub fn new(target_frame_rate: u32) -> Self {
        Self {
            target_frame_rate,
            sleep_duration: Duration::ZERO,
            fps: Fps::default(),
        }
    }

    /// Sleeps for an adaptive amount of time if the frame rate is higher than the target frame rate.
    pub fn step(&mut self) {
        const STEP_SIZE: Duration = Duration::from_micros(100);

        self.fps.tick();
        if self.fps.avg() > self.target_frame_rate as f64 {
            self.sleep_duration += STEP_SIZE;
            std::thread::sleep(self.sleep_duration);
        } else if self.sleep_duration < STEP_SIZE {
            // Ensure sleep duration does not go below zero
            self.sleep_duration = Duration::ZERO;
        } else if self.sleep_duration > Duration::ZERO {
            self.sleep_duration -= STEP_SIZE;
        }
    }
}
