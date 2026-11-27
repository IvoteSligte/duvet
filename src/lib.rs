use std::thread::JoinHandle;
use std::time::Instant;

use nvenc::bitstream::AsyncBitStream;
use nvenc::sys::enums::{NVencBufferFormat, NVencPicType};
use tracing::{debug, error, info, warn};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
// TODO: rewrite windows_capture so it does not depend on rayon to reduce the number of (unused) dependencies
use windows_capture::{dxgi_duplication_api::DxgiDuplicationApi, monitor::Monitor};

use downscaler::Downscaler;
use encoder::Encoder;
use util::{FrameRateLimiter, SyncQueue};

mod downscaler;
mod encoder;
mod error;
mod util;

pub use error::Error;

/// The returned H.264 video stream has a resolution of at most `resolution` pixels
/// and a frame-rate of roughly `frame_rate` FPS.
///
/// Each frame consists of the encoded bytes and a boolean indicating if the frame is a keyframe.
pub fn capture(
    resolution: (u32, u32),
    frame_rate: u32,
    bitrate: u32,
) -> Result<impl Iterator<Item = Result<(Vec<u8>, bool), Error>>, Error> {
    let monitor = Monitor::primary()?;
    let duplication = DxgiDuplicationApi::new(monitor)?;
    let (mut queue, aux_queue) = SyncQueue::new(NUM_BUFFERS);
    let handle = std::thread::spawn(move || {
        run_encoder(duplication, aux_queue, resolution, frame_rate, bitrate)
    });
    // First NUM_BUFFERS buffers are uninitialized.
    for _ in 0..NUM_BUFFERS {
        let output = queue.pop().map_err(|_| Error::ChannelDisconnected)?;
        queue.push(output).map_err(|_| Error::ChannelDisconnected)?;
    }
    Ok(Iter {
        handle: Some(handle),
        queue,
    })
}

pub struct ScreenInfo {
    pub width: u32,
    pub height: u32,
    pub frame_rate: u32,
}

pub fn screen_info() -> Result<ScreenInfo, Error> {
    let monitor = Monitor::primary()?;
    Ok(ScreenInfo {
        width: monitor.width()?,
        height: monitor.height()?,
        frame_rate: monitor.refresh_rate()?,
    })
}

// More than one buffer is theoretically more performant, but I have not observed any performance issues
// and it seems to randomly deadlock with more than one buffer.
const NUM_BUFFERS: usize = 1;

// NOTE: cursor movements also generate frames, should these frames be ignored? since they bloat the FPS
// can also be resolved by disabling the cursor and drawing it manually...

fn run_encoder(
    mut duplication: DxgiDuplicationApi,
    mut queue: SyncQueue<AsyncBitStream>,
    resolution: (u32, u32),
    frame_rate: u32,
    bitrate: u32,
) -> Result<(), Error> {
    info!("Spawned encoder thread");

    let device = duplication.device();
    let mut encoder = Encoder::new(device, resolution, frame_rate, bitrate)?;
    let downscaler = Downscaler::new(
        device.clone(),
        duplication.device_context().clone(),
        DXGI_FORMAT_B8G8R8A8_UNORM,
        resolution,
    )?;
    for _ in 0..NUM_BUFFERS {
        queue
            .push(encoder.create_output_buffer()?)
            .map_err(|_| Error::ChannelDisconnected)?;
    }
    let mut frame_rate_limiter = FrameRateLimiter::new(frame_rate);
    for i in 0.. {
        let output_buffer = queue.pop().map_err(|_| Error::ChannelDisconnected)?;
        frame_rate_limiter.step();
        let frame = duplication.acquire_next_frame(u32::MAX)?;
        let downscaled_texture = downscaler.downscale(frame.texture())?;
        encoder.encode(
            i,
            NVencBufferFormat::ARGB,
            downscaled_texture,
            &output_buffer,
        )?;
        queue
            .push(output_buffer)
            .map_err(|_| Error::ChannelDisconnected)?;
    }
    Ok(())
}

struct Iter {
    handle: Option<JoinHandle<Result<(), Error>>>,
    queue: SyncQueue<AsyncBitStream>,
}

impl Iterator for Iter {
    type Item = Result<(Vec<u8>, bool), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let Ok(output) = self.queue.pop() else {
            return None;
        };
        let before_lock = Instant::now();
        let result = match output.try_lock() {
            Ok(lock) => {
                let picture_type = lock.picture_type();
                if picture_type == NVencPicType::UNKNOWN {
                    warn!("Unknown NVENC picture type generated.");
                }
                let is_keyframe = picture_type == NVencPicType::IDR;
                let data = lock.as_slice().to_vec();
                debug!(
                    "Read {} bytes of data in {:?} (type: {:?})",
                    data.len(),
                    before_lock.elapsed(),
                    picture_type,
                );
                Ok((data, is_keyframe))
            }
            Err(err) => Err(err.into()),
        };
        if let Err(_) = self.queue.push(output) {
            warn!("Encoder thread exited");
            return None;
        }
        Some(result)
    }
}

impl Drop for Iter {
    fn drop(&mut self) {
        match self.handle.take().unwrap().join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                error!("The encoder thread raised an error: {err}");
            }
            Err(join_err) => {
                error!("Failed to join encoder thread");
                std::panic::resume_unwind(join_err);
            }
        }
    }
}
