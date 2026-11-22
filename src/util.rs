use std::sync::mpsc;
use std::time::Duration;

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
