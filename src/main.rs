use std::time::Instant;

use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
// TODO: rewrite windows_capture so it does not depend on rayon to reduce the number of (unused) dependencies
use windows_capture::{dxgi_duplication_api::DxgiDuplicationApi, monitor::Monitor};

use downscaler::Downscaler;

mod downscaler;
mod encoder;
mod error;
mod util;

pub use error::Error;

const RESOLUTION: (u32, u32) = (1920, 1080);

fn main() -> Result<(), Error> {
    let monitor = Monitor::primary()?;
    let mut duplication = DxgiDuplicationApi::new(monitor)?;
    #[cfg(feature = "downscale")]
    let downscaler = Downscaler::new(
        duplication.device().clone(),
        duplication.device_context().clone(),
        DXGI_FORMAT_B8G8R8A8_UNORM,
        RESOLUTION,
    )?;
    let start_instant = Instant::now();
    let mut prev_instant = Instant::now();
    for i in 0..300 {
        let frame = duplication.acquire_next_frame(u32::MAX)?;
        #[cfg(feature = "downscale")]        
        let downscaled_texture = downscaler.downscale(frame.texture())?;
        let now = Instant::now();
        println!("Frame {i} at {:?} with {:?} latency", now - start_instant, now - prev_instant);
        prev_instant = now;
    }
    println!("Average FPS: {:.1}", 300.0 / start_instant.elapsed().as_secs_f64());
    println!("Downscaling enabled: {}", cfg!(feature = "downscale"));
    Ok(())
}
