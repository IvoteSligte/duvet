use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tracing::{debug, error, info, warn};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
// TODO: rewrite windows_capture so it does not depend on rayon to reduce the number of (unused) dependencies
use windows_capture::{dxgi_duplication_api::{DxgiDuplicationApi, DxgiDuplicationFrame}, monitor::Monitor};
use ::nvenc::bitstream::BitStream;
use ::nvenc::sys::enums::{NVencPicType, NVencBufferFormat};

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
    let timeout = Duration::from_secs(1) / frame_rate + Duration::from_millis(1);
    let (mut queue, aux_queue) = SyncQueue::new(NUM_BUFFERS, timeout);
    let handle = std::thread::spawn(move || run_encoder(
        duplication,
        aux_queue,
        resolution,
        frame_rate,
        bitrate,
        timeout,
    ));
    // First NUM_BUFFERS buffers are uninitialized.
    for _ in 0..NUM_BUFFERS {
        let output = loop {
            match queue.pop() {
                Ok(output) => break output,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    debug!("Timed out waiting for initial buffer");
                }
                Err(_) => return Err(Error::ChannelDisconnected),
            }
        };
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
    mut queue: SyncQueue<BitStream>,
    resolution: (u32, u32),
    frame_rate: u32,
    bitrate: u32,
    timeout: Duration,
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
        let output_buffer = acquire_output_buffer(&mut queue, i)?;
        frame_rate_limiter.step();
        let frame = loop {
            match duplication.acquire_next_frame(timeout.as_millis() as _) {
                Ok(frame) => break frame,
                Err(windows_capture::dxgi_duplication_api::Error::Timeout) => {
                    // This can happen in a normal situation when the captured screen is static.
                    debug!("Timed out while capturing frame {i}");
                }
                Err(err) => {
                    error!("Failed to capture frame {i}: {err}");
                    return Err(Error::DxgiDuplication(err));
                }
            }            
        };
        let downscaled_texture = downscaler.downscale(frame.texture())?;
        encoder.encode(i, NVencBufferFormat::ARGB, downscaled_texture, &output_buffer)?;
        queue
            .push(output_buffer)
            .map_err(|_| Error::ChannelDisconnected)?;
    }
    Ok(())
}

fn acquire_output_buffer(queue: &mut SyncQueue<BitStream>, frame_index: usize) -> Result<BitStream, Error> {
    loop {
        match queue.pop() {
            Ok(output) => return Ok(output),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                debug!("Timed out waiting for empty buffer for frame {frame_index}");
            }
            Err(_) => {
                warn!("Failed to pop empty buffer from queue");
                return Err(Error::ChannelDisconnected);
            }
        }
    }    
}

struct Iter {
    handle: Option<JoinHandle<Result<(), Error>>>,
    queue: SyncQueue<BitStream>,
}

impl Iterator for Iter {
    type Item = Result<(Vec<u8>, bool), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let output = loop {
            match self.queue.pop() {
                Ok(output) => break output,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    debug!("Timed out waiting for full buffer");
                }
                Err(_) => return None,
            }
        };
        let before_lock = Instant::now();
        // try_lock(wait) should have wait=true, since we use the synchronous NVenc mode.
        let result = match output.try_lock(true) {
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
