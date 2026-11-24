use std::fs::File;
use std::io::Write;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const RESOLUTION: (u32, u32) = (1920, 1080);
const FRAME_RATE: u32 = 60;
const BITRATE: u32 = 8 << 20; // 1 MB

fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::level_filters::LevelFilter::DEBUG.into())
                .from_env_lossy(),
        )
        .with(tracing_subscriber::fmt::layer().without_time())
        .init();

    let mut file = File::create("capture.h264").unwrap();

    for frame_result in duvet::capture(RESOLUTION, FRAME_RATE, BITRATE).unwrap() {
        let (bytes, _is_keyframe) = frame_result.unwrap();
        file.write_all(&bytes).unwrap();
    }
}
