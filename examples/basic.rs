const RESOLUTION: (u32, u32) = (1920, 1080);
const FRAME_RATE: u32 = 60;
const BITRATE: u32 = 8 << 20; // 1 MB

fn main() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::level_filters::LevelFilter::DEBUG.into())
                .from_env_lossy(),
        )
        .with(tracing_subscriber::fmt::layer().without_time())
        .init();

    for (i, frame_result) in duvet::capture(RESOLUTION, FRAME_RATE, BITRATE)
        .unwrap()
        .enumerate()
    {
        let (bytes, is_keyframe) = frame_result.unwrap();
        println!(
            "Captured frame {i}: bytes: {}, is keyframe: {}",
            bytes.len(),
            is_keyframe
        );
    }
}
