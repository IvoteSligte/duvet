use std::fs::File;
use std::io::Write;
use std::time::Instant;

use nvenc::bitstream::BitStream;
use nvenc::session::InitParams;
use nvenc::{
    session::Session,
    sys::{
        enums::*,
        guids::{NV_ENC_CODEC_H264_GUID, NV_ENC_PRESET_P3_GUID},
    },
};
use nvenc::encoder::Encoder;
use windows::Win32::{
    Graphics::{
        Direct3D11::*,
        Dxgi::Common::*,
    },
};
use windows::core::Result;
use windows_capture::dxgi_duplication_api::DxgiDuplicationApi;
use windows_capture::monitor::Monitor;

fn image(device: &ID3D11Device) -> Result<ID3D11Texture2D> {
    unsafe {
        let mut texture = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: 1920,
            Height: 1080,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: DXGI_CPU_ACCESS_NONE,
            MiscFlags: 0,
        };
        device.CreateTexture2D(&raw const desc, None, Some(&mut texture))?;
        Ok(texture.unwrap())
    }
}

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

fn create_encoder(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    frame_rate: u32,
    average_bitrate: u32,
) -> Encoder {
    let gcd = gcd(width, height);
    let aspect_ratio = [width / gcd, height / gcd];
    let session: Session<nvenc::session::NeedsConfig> = Session::open_dx(device).unwrap();
    assert!(
        session
            .get_encode_codecs()
            .unwrap()
            .contains(&NV_ENC_CODEC_H264_GUID)
    );
    assert!(
        session
            .get_encode_presets(NV_ENC_CODEC_H264_GUID)
            .unwrap()
            .contains(&NV_ENC_PRESET_P3_GUID)
    );
    let (session, mut config) = session
        .get_encode_preset_config_ex(
            NV_ENC_CODEC_H264_GUID,
            NV_ENC_PRESET_P3_GUID,
            nvenc::sys::enums::NVencTuningInfo::LowLatency,
        )
        .unwrap();
    config.preset_cfg.rc_params.rate_control_mode = NVencParamsRcMode::VBR;
    config.preset_cfg.rc_params.average_bit_rate = average_bitrate;
    config.preset_cfg.gop_len = 0xffffffff;
    config.preset_cfg.frame_interval_p = 1;
    println!("P-Frames: {}", config.preset_cfg.frame_interval_p);
    println!("Bitrate: {}", config.preset_cfg.rc_params.average_bit_rate);
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
    session.init_encoder(init_params).unwrap()
}

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const FRAME_RATE: u32 = 75;
const BITRATE: u32 = 10_000_000;
const TIMEOUT: u32 = 10000;

fn main() {
    let monitor = Monitor::primary().unwrap();
    let mut duplication = DxgiDuplicationApi::new(monitor).unwrap();

    let encoder = create_encoder(duplication.device(), WIDTH, HEIGHT, FRAME_RATE, BITRATE);
    let (processed, to_use) = std::sync::mpsc::sync_channel::<BitStream>(2);
    let (re_use, to_process) = std::sync::mpsc::sync_channel::<BitStream>(2);
    processed
        .send(encoder.create_bitstream_buffer().unwrap())
        .unwrap();
    processed
        .send(encoder.create_bitstream_buffer().unwrap())
        .unwrap();

    let handle = std::thread::spawn({
        move || {
            let mut out = File::create("output.h264").unwrap();

            while let Ok(output) = to_process.recv() {
                let lock = output.try_lock(true).unwrap();
                out.write_all(lock.as_slice()).unwrap();
                drop(lock);
                if let Err(_) = processed.send(output) {
                    break;
                }
            }
        }
    });
    let instant = Instant::now();
    let mut i = 0;
    while i < 600 {
        if let Ok(output) = to_use.recv() {
            println!("Frame: {i}");
            let frame = duplication.acquire_next_frame(TIMEOUT).unwrap();
            let texture = frame.texture();
            let registered = encoder
                .register_resource_dx11(&texture, NVencBufferFormat::ARGB, 0)
                .unwrap();

            encoder
                .encode_picture(
                    &registered,
                    &output,
                    i,
                    instant.elapsed().as_millis() as u64,
                    NVencBufferFormat::ARGB,
                    NVencPicStruct::Frame,
                    NVencPicType::P,
                    None,
                )
                .unwrap();
            re_use.send(output).unwrap();
            i += 1;
        }
    }

    drop(to_use);
    drop(re_use);
    handle.join().unwrap();
}
