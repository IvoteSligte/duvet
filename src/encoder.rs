use ::nvenc::{
    bitstream::BitStream,
    encoder::Encoder as NVencEncoder,
    session::{InitParams, NeedsConfig, Session},
    sys::{
        enums::*,
        guids::{NV_ENC_CODEC_H264_GUID, NV_ENC_PRESET_P3_GUID},
    },
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};

use std::time::Instant;

use crate::Error;

pub struct Encoder {
    inner: NVencEncoder,
    frame_rate: u32,
    start_instant: Option<Instant>,
}

impl Encoder {
    pub fn new(
        device: &ID3D11Device,
        (width, height): (u32, u32),
        frame_rate: u32,
        average_bitrate: u32,
    ) -> Result<Self, Error> {
        let gcd = crate::util::gcd(width, height);
        let aspect_ratio = [width / gcd, height / gcd];
        let session: Session<NeedsConfig> = Session::open_dx(device).unwrap();
        if !session
            .get_encode_codecs()?
            .contains(&NV_ENC_CODEC_H264_GUID)
            || !session
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
        config.preset_cfg.rc_params.rate_control_mode = NVencParamsRcMode::CBR;
        config.preset_cfg.rc_params.average_bit_rate = average_bitrate;
        config.preset_cfg.gop_len = 0xffffffff;
        config.preset_cfg.frame_interval_p = 1;

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
        Ok(Self {
            inner: session.init_encoder(init_params)?,
            frame_rate,
            start_instant: None,
        })
    }

    pub fn create_output_buffer(&self) -> Result<BitStream, Error> {
        Ok(self.inner.create_bitstream_buffer()?)
    }

    pub fn encode(
        &mut self,
        frame_index: usize,
        input_format: NVencBufferFormat,
        input_texture: &ID3D11Texture2D,
        output_buffer: &BitStream,
    ) -> Result<(), Error> {
        // TODO: register all the possible input textures in advance
        let registered =
            self.inner
                .register_resource_dx11(input_texture, NVencBufferFormat::ARGB, 0)?;
        // TODO: provide an option for the user to manually insert IDRs instead of doing it automatically every second
        let flags = match frame_index % self.frame_rate as usize {
            0 => NVencPicFlags::FORCE_IDR,
            _ => NVencPicFlags::empty(),
        };
        let start_instant = *self.start_instant.get_or_insert_with(|| Instant::now());
        self.inner.encode_picture(
            &registered,
            output_buffer,
            frame_index as _,
            start_instant.elapsed().as_millis() as u64,
            input_format,
            NVencPicStruct::Frame,
            NVencPicType::P,
            flags,
            None,
        )?;
        Ok(())
    }
}
