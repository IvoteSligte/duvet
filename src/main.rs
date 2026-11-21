use std::time::Instant;
use std::sync::mpsc;

use nvenc::bitstream::BitStream;
use nvenc::session::InitParams;
use nvenc::{
    session::Session,
    sys::{
        enums::*,
        guids::{NV_ENC_CODEC_H264_GUID, NV_ENC_PRESET_P3_GUID},
    },
};
use nvenc::encoder::Encoder as NvEncoder;
use windows::Win32::{
    Graphics::{
        Direct3D11::*,
    },
};
use windows_capture::dxgi_duplication_api::DxgiDuplicationApi;
use windows_capture::monitor::Monitor;

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
) -> NvEncoder {
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

struct SyncQueue<T> {
    sender: mpsc::SyncSender<T>,
    receiver: mpsc::Receiver<T>,
}

impl<T> SyncQueue<T> {
    pub fn new(capacity: usize) -> (Self, Self) {
        // Create bidirectional channel as two unidirectional channels
        let (a_sender, b_receiver) = mpsc::sync_channel(capacity);
        let (b_sender, a_receiver) = mpsc::sync_channel(capacity);
        let a = Self { sender: a_sender, receiver: a_receiver };
        let b = Self { sender: b_sender, receiver: b_receiver };
        (a, b)
    }

    pub fn push(&mut self, value: T) -> Result<(), mpsc::SendError<T>> {
        self.sender.send(value)
    }

    pub fn pop(&mut self) -> Result<T, mpsc::RecvError> {
        self.receiver.recv()
    }
}

// TODO: resize if input resolution is different from configured resolution
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const FRAME_RATE: u32 = 75;
const BITRATE: u32 = 10_000_000;
const TIMEOUT: u32 = 10000;
const NUM_BUFFERS: usize = 2;

fn spawn_encoder_thread(
    mut duplication: DxgiDuplicationApi,
    mut queue: SyncQueue<BitStream>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn({
        move || {
            let encoder = create_encoder(duplication.device(), WIDTH, HEIGHT, FRAME_RATE, BITRATE);
            for _ in 0..NUM_BUFFERS {
                queue
                    .push(encoder.create_bitstream_buffer().unwrap())
                    .unwrap();
            }
            let instant = Instant::now();
            let mut i = 0;
            while i < 600 {
                if let Ok(output) = queue.pop() {
                    println!("Frame: {i}");
                    let frame = duplication.acquire_next_frame(TIMEOUT).unwrap();
                    // NOTE: should this texture be copied to another texture before giving it to the encoder?
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
                    queue.push(output).unwrap();
                    i += 1;
                }
            }
            println!("Average FPS: {:?}", 600.0 / instant.elapsed().as_secs_f64());
        }
    })
}

fn main() {
    let monitor = Monitor::primary().unwrap();
    let duplication = DxgiDuplicationApi::new(monitor).unwrap();
    let (mut queue, aux_queue) = SyncQueue::new(NUM_BUFFERS);
    let handle = spawn_encoder_thread(duplication, aux_queue);
    // First NUM_BUFFERS buffers are uninitialized.
    for _ in 0..NUM_BUFFERS {
        let output = queue.pop().unwrap();
        queue.push(output).unwrap();
    }
    while let Ok(output) = queue.pop() {
        let lock = output.try_lock(true).unwrap();
        let data = lock.as_slice().to_vec();
        drop(lock);
        println!("{} bytes of data", data.len());
        if let Err(_) = queue.push(output) {
            break;
        }
    }
    handle.join().unwrap();
}
