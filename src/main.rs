use std::time::Instant;

use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
// TODO: rewrite windows_capture so it does not depend on rayon to reduce the number of (unused) dependencies
use nvenc::sys::enums::NVencBufferFormat;
use windows_capture::{dxgi_duplication_api::DxgiDuplicationApi, monitor::Monitor};

use downscaler::Downscaler;
use encoder::Encoder;
use util::SyncQueue;

mod downscaler;
mod encoder;
mod error;
mod util;

pub use error::Error;

const RESOLUTION: (u32, u32) = (1920, 1080);
const BITRATE: u32 = 8_000_000;
const NUM_BUFFERS: usize = 3;
const NUM_FRAMES: usize = 300;

// TODO: frame-rate limiter
// TODO: test with multiple downscale output textures
// TODO: test without console writes every frame as that can be a bottleneck on Windows

fn main() -> Result<(), Error> {
    let monitor = Monitor::primary()?;
    let frame_rate = monitor.refresh_rate()?;
    let mut duplication = DxgiDuplicationApi::new(monitor)?;
    let downscaler = Downscaler::new(
        duplication.device().clone(),
        duplication.device_context().clone(),
        DXGI_FORMAT_B8G8R8A8_UNORM,
        RESOLUTION,
    )?;
    let mut encoder = Encoder::new(duplication.device(), RESOLUTION, frame_rate, BITRATE)?;
    let (mut queue, mut queue2) = SyncQueue::new(NUM_BUFFERS);
    for _ in 0..NUM_BUFFERS {
        queue2.push(encoder.create_output_buffer()?)?;
    }
    let handle = std::thread::spawn(move || -> Result<(), Error> {
        let mut queue = queue2;
        for i in 0..NUM_FRAMES {
            let buffer = queue.pop()?;
            let lock = buffer.try_lock()?;
            let encoded_bytes = lock.as_slice().to_vec();
            drop(lock);
            queue.push(buffer)?;
            println!("Frame {i} encoded to {} bytes", encoded_bytes.len());
        }
        Ok(())
    });
    let start_instant = Instant::now();
    let mut prev_instant = Instant::now();
    for i in 0..NUM_FRAMES {
        let frame = duplication.acquire_next_frame(u32::MAX)?;
        let downscaled_texture = downscaler.downscale(frame.texture())?;
        {
            let output_buffer = queue.pop()?;
            encoder.encode(
                i,
                NVencBufferFormat::ARGB,
                downscaled_texture,
                &output_buffer,
            )?;
            queue.push(output_buffer)?;
        }
        let now = Instant::now();
        println!(
            "Frame {i} at {:?} with {:?} since previous frame",
            now - start_instant,
            now - prev_instant
        );
        prev_instant = now;
    }
    match handle.join() {
        Ok(Ok(())) => {}
        Ok(Err(err)) => return Err(err),
        Err(join_err) => std::panic::resume_unwind(join_err),
    }
    println!(
        "Average FPS: {:.1}",
        300.0 / start_instant.elapsed().as_secs_f64()
    );
    Ok(())
}
