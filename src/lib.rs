use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ::nvenc::{
    bitstream::BitStream,
    encoder::Encoder as NVEncoder,
    session::{InitParams, NeedsConfig, Session},
    sys::{
        enums::*,
        guids::{NV_ENC_CODEC_H264_GUID, NV_ENC_PRESET_P3_GUID},
    },
};
use tracing::{debug, error, info, warn};
use windows::Win32::Graphics::Direct3D11::*;
use windows_capture::{dxgi_duplication_api::DxgiDuplicationApi, monitor::Monitor};

use downscaler::downscale;
use util::{FrameRateLimiter, SyncQueue, gcd};

mod downscaler;
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
    let handle = spawn_encoder_thread(
        duplication,
        aux_queue,
        resolution,
        frame_rate,
        bitrate,
        timeout,
    );
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

fn create_encoder(
    device: &ID3D11Device,
    (width, height): (u32, u32),
    frame_rate: u32,
    average_bitrate: u32,
) -> Result<NVEncoder, Error> {
    let gcd = gcd(width, height);
    let aspect_ratio = [width / gcd, height / gcd];
    let session: Session<NeedsConfig> = Session::open_dx(device).unwrap();
    if !session
        .get_encode_codecs()?
        .contains(&NV_ENC_CODEC_H264_GUID)
    {
        return Err(Error::UnsupportedCodec);
    }
    if !session
        .get_encode_presets(NV_ENC_CODEC_H264_GUID)?
        .contains(&NV_ENC_PRESET_P3_GUID)
    {
        return Err(Error::UnsupportedCodec);
    }
    let (session, mut config) = session.get_encode_preset_config_ex(
        NV_ENC_CODEC_H264_GUID,
        NV_ENC_PRESET_P3_GUID,
        NVencTuningInfo::LowLatency,
    )?;
    config.preset_cfg.rc_params.rate_control_mode = NVencParamsRcMode::VBR;
    config.preset_cfg.rc_params.average_bit_rate = average_bitrate;
    config.preset_cfg.gop_len = 0xffffffff;
    config.preset_cfg.frame_interval_p = 1;
    info!("P-Frames: {}", config.preset_cfg.frame_interval_p);
    info!("Bitrate: {}", config.preset_cfg.rc_params.average_bit_rate);

    let init_params = InitParams {
        encode_guid: NV_ENC_CODEC_H264_GUID,
        preset_guid: NV_ENC_PRESET_P3_GUID,
        aspect_ratio: aspect_ratio,
        encode_config: &mut config.preset_cfg,
        tuning_info: NVencTuningInfo::LowLatency,
        buffer_format: NVencBufferFormat::ARGB,
        frame_rate: [frame_rate, 1],
        resolution: [width, height],
        enable_ptd: true,
        max_encoder_resolution: [0, 0],
    };
    Ok(session.init_encoder(init_params)?)
}

// More than one buffer is theoretically more performant, but I have not observed any performance issues
// and it seems to randomly deadlock with more than one buffer.
const NUM_BUFFERS: usize = 1;

// NOTE: cursor movements also generate frames, should these frames be ignored? since they bloat the FPS
// can also be resolved by disabling the cursor and drawing it manually...

fn spawn_encoder_thread(
    mut duplication: DxgiDuplicationApi,
    mut queue: SyncQueue<BitStream>,
    resolution: (u32, u32),
    frame_rate: u32,
    bitrate: u32,
    timeout: Duration,
) -> JoinHandle<Result<(), Error>> {
    std::thread::spawn({
        move || -> Result<(), Error> {
            info!("Spawned encoder thread");

            let encoder = create_encoder(duplication.device(), resolution, frame_rate, bitrate)?;
            for _ in 0..NUM_BUFFERS {
                queue
                    .push(encoder.create_bitstream_buffer()?)
                    .map_err(|_| Error::ChannelDisconnected)?;
            }
            let mut frame_rate_limiter = FrameRateLimiter::new(frame_rate);
            let start_instant = Instant::now();
            for i in 0.. {
                let output = loop {
                    match queue.pop() {
                        Ok(output) => break output,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            debug!("Timed out waiting for empty buffer for frame {i}");
                        }
                        Err(_) => {
                            debug!("Failed to pop empty buffer from queue");
                            return Err(Error::ChannelDisconnected);
                        }
                    }
                };
                frame_rate_limiter.step();
                let frame = loop {
                    match duplication.acquire_next_frame(timeout.as_millis() as _) {
                        Ok(frame) => break frame,
                        Err(windows_capture::dxgi_duplication_api::Error::Timeout) => {
                            println!("Timed out while capturing frame {i}");
                        }
                        Err(err) => panic!("Failed to capture frame {i}: {err}"),
                    }
                };
                let texture = downscale(
                    frame.device(),
                    frame.device_context(),
                    frame.texture(),
                    resolution,
                )?;
                let registered =
                    encoder.register_resource_dx11(&texture, NVencBufferFormat::ARGB, 0)?;
                encoder.encode_picture(
                    &registered,
                    &output,
                    i,
                    start_instant.elapsed().as_millis() as u64,
                    NVencBufferFormat::ARGB,
                    NVencPicStruct::Frame,
                    NVencPicType::P,
                    None,
                )?;
                queue.push(output).map_err(|_| Error::ChannelDisconnected)?;
            }
            Ok(())
        }
    })
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
                let is_keyframe = lock.picture_type() == NVencPicType::IDR;
                let data = lock.as_slice().to_vec();
                debug!(
                    "Read {} bytes of data in {:?}",
                    data.len(),
                    before_lock.elapsed()
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
