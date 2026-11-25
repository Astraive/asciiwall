use std::{io::Cursor, time::Duration};

use ascii_core::{Budget, CancellationToken, Error, MalformedReason, UnsupportedReason};
use ascii_media::{AnimationLoops, DecodeLimits, DecodeResources, decode_animation, decode_image};
use sha2::{Digest, Sha256};

const APNG_SETTINGS: &str = "png=0.18.0;canvas=2x1;frames=5;sample=rgba8;frame0=full:[10,20,30,255,0,255,0,255],dispose=none,blend=source,delay=0/100;frame1=rect(1,0,1,1):[255,0,0,128],dispose=previous,blend=over,delay=1/100;frame2=rect(1,0,1,1):[0,255,0,255],dispose=none,blend=source,delay=1/100;frame3=rect(0,0,1,1):[255,255,0,255],dispose=background,blend=source,delay=1/100;frame4=rect(1,0,1,1):[0,0,255,128],dispose=none,blend=over,delay=1/100;plays=2";
const GIF_SETTINGS: &str = "gif=0.14.0;canvas=2x1;frames=4;palette=0:black,1:red,2:green,3:blue;frame0=full:[1,1],keep,delay=0;frame1=rect(1,0,1,1):[2],previous,delay=1;frame2=rect(1,0,1,1):[3],background,delay=2;frame3=rect(0,0,1,1):[0],transparent=0,any,delay=0;repeat=1";
fn assert_sha256(bytes: &[u8], expected: &str) {
    assert_eq!(format!("{:x}", Sha256::digest(bytes)), expected);
}

fn partial_disposal_gif(repeat: Option<gif::Repeat>) -> Vec<u8> {
    let palette = [0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255];
    let mut encoded = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut encoded, 2, 1, &palette).unwrap();
        if let Some(repeat) = repeat {
            encoder.set_repeat(repeat).unwrap();
        }
        let mut frame = gif::Frame::from_indexed_pixels(2, 1, vec![1, 1], None);
        frame.delay = 0;
        encoder.write_frame(&frame).unwrap();

        let mut frame = gif::Frame::from_indexed_pixels(1, 1, vec![2], None);
        frame.left = 1;
        frame.dispose = gif::DisposalMethod::Previous;
        frame.delay = 1;
        encoder.write_frame(&frame).unwrap();

        let mut frame = gif::Frame::from_indexed_pixels(1, 1, vec![3], None);
        frame.left = 1;
        frame.dispose = gif::DisposalMethod::Background;
        frame.delay = 2;
        encoder.write_frame(&frame).unwrap();

        let mut frame = gif::Frame::from_indexed_pixels(1, 1, vec![0], Some(0));
        frame.dispose = gif::DisposalMethod::Any;
        frame.delay = 0;
        encoder.write_frame(&frame).unwrap();
    }
    encoded
}

fn limits(resources: DecodeResources) -> DecodeLimits {
    DecodeLimits::new(
        Budget::new(1 << 20, 1 << 20, 1 << 20),
        resources,
        8,
        8,
        1 << 20,
        1 << 20,
    )
}

fn partial_rect_previous_apng(play_count: u32) -> Vec<u8> {
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(5, play_count).unwrap();
        encoder.validate_sequence(true);
        let mut writer = encoder.write_header().unwrap();
        writer.set_frame_delay(0, 100).unwrap();
        writer.set_dispose_op(png::DisposeOp::None).unwrap();
        writer.set_blend_op(png::BlendOp::Source).unwrap();
        writer
            .write_image_data(&[10, 20, 30, 255, 0, 255, 0, 255])
            .unwrap();

        writer.set_frame_dimension(1, 1).unwrap();
        writer.set_frame_position(1, 0).unwrap();
        writer.set_frame_delay(1, 100).unwrap();
        writer.set_dispose_op(png::DisposeOp::Previous).unwrap();
        writer.set_blend_op(png::BlendOp::Over).unwrap();
        writer.write_image_data(&[255, 0, 0, 128]).unwrap();

        writer.reset_frame_position().unwrap();
        writer.set_frame_dimension(1, 1).unwrap();
        writer.set_frame_position(1, 0).unwrap();
        writer.set_frame_delay(1, 100).unwrap();
        writer.set_dispose_op(png::DisposeOp::None).unwrap();
        writer.set_blend_op(png::BlendOp::Source).unwrap();
        writer.write_image_data(&[0, 255, 0, 255]).unwrap();

        writer.reset_frame_position().unwrap();
        writer.set_frame_dimension(1, 1).unwrap();
        writer.set_frame_position(0, 0).unwrap();
        writer.set_frame_delay(1, 100).unwrap();
        writer.set_dispose_op(png::DisposeOp::Background).unwrap();
        writer.set_blend_op(png::BlendOp::Source).unwrap();
        writer.write_image_data(&[255, 255, 0, 255]).unwrap();

        writer.reset_frame_position().unwrap();
        writer.set_frame_dimension(1, 1).unwrap();
        writer.set_frame_position(1, 0).unwrap();
        writer.set_frame_delay(1, 100).unwrap();
        writer.set_dispose_op(png::DisposeOp::None).unwrap();
        writer.set_blend_op(png::BlendOp::Over).unwrap();
        writer.write_image_data(&[0, 0, 255, 128]).unwrap();
        writer.finish().unwrap();
    }
    encoded
}

#[test]
fn apng_subframes_compose_full_canvas_and_share_a_bounded_timeline() {
    let resources = DecodeResources::new(1, 1 << 20);
    let encoded = partial_rect_previous_apng(2);
    println!(
        "APNG config_sha256={:x} settings={APNG_SETTINGS}",
        Sha256::digest(APNG_SETTINGS.as_bytes())
    );
    println!("APNG fixture_sha256={:x}", Sha256::digest(&encoded));
    assert_sha256(
        &encoded,
        "9a2efd0e082adadd8e166d92a8dd2724bf27484378375c9fac7fa91d5bd5e079",
        APNG_SETTINGS.as_bytes(),
        "4960c500b02bfa20654df299568b29dea2554e0ace6299284b8e9e175e82f28f",
    );
    assert_sha256(
        &encoded,
        "9a2efd0e082adadd8e166d92a8dd2724bf27484378375c9fac7fa91d5bd5e079",
    );

    let animation = decode_animation(
        &mut Cursor::new(&encoded),
        &limits(resources.clone()),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(animation.frames().len(), 5);
    assert_eq!(animation.loops(), AnimationLoops::Finite(2));
    assert_eq!(animation.loop_duration(), Duration::from_millis(50));
    assert_eq!(animation.frames()[0].duration(), Duration::from_millis(10));
    assert_eq!(
        animation.frames()[0].image().pixels(),
        &[10, 20, 30, 255, 0, 255, 0, 255]
    );
    assert_eq!(
        animation.frames()[1].image().pixels(),
        &[10, 20, 30, 255, 128, 127, 0, 255]
    );
    assert_eq!(
        animation.frames()[2].image().pixels(),
        &[10, 20, 30, 255, 0, 255, 0, 255]
    );
    assert_eq!(
        animation.frames()[3].image().pixels(),
        &[255, 255, 0, 255, 0, 255, 0, 255]
    );
    assert_eq!(
        animation.frames()[4].image().pixels(),
        &[0, 0, 0, 0, 0, 127, 128, 255]
    );
    assert_eq!(
        animation.frame_at(Duration::ZERO).image().pixels(),
        animation.frames()[0].image().pixels()
    );
    assert_eq!(
        animation
            .frame_at(Duration::from_millis(10))
            .image()
            .pixels(),
        animation.frames()[1].image().pixels()
    );
    assert_eq!(
        animation
            .frame_at(Duration::from_millis(20))
            .image()
            .pixels(),
        animation.frames()[2].image().pixels()
    );
    assert_eq!(
        animation
            .frame_at(Duration::from_millis(40))
            .image()
            .pixels(),
        animation.frames()[4].image().pixels()
    );
    assert_eq!(
        animation
            .frame_at(Duration::from_millis(100))
            .image()
            .pixels(),
        animation.frames()[4].image().pixels()
    );
    assert!(resources.cpu_bytes_in_use() > 0);
    drop(animation);
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}

#[test]
fn malformed_apng_is_rejected_without_retaining_canvas_memory() {
    let resources = DecodeResources::new(1, 1 << 20);
    let mut encoded = partial_rect_previous_apng(2);
    let end = encoded.len();
    encoded.truncate(end.saturating_sub(5));
    assert!(matches!(
        decode_animation(
            &mut Cursor::new(&encoded),
            &limits(resources.clone()),
            &CancellationToken::new(),
        ),
        Err(Error::Malformed(MalformedReason::InvalidEncodedData))
    ));
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}

#[test]
fn static_decode_never_publishes_an_apng_subframe() {
    let encoded = partial_rect_previous_apng(2);
    assert!(matches!(
        decode_image(
            &mut Cursor::new(&encoded),
            &limits(DecodeResources::new(1, 1 << 20)),
            &CancellationToken::new(),
        ),
        Err(Error::Unsupported(UnsupportedReason::AnimatedImage))
    ));
}

fn update_chunk_crc(bytes: &mut [u8], chunk_start: usize) {
    let chunk_len = u32::from_be_bytes(
        bytes[chunk_start..chunk_start + 4]
            .try_into()
            .expect("four-byte PNG chunk length"),
    ) as usize;
    let crc_start = chunk_start + 8 + chunk_len;
    let mut crc = !0_u32;
    for byte in &bytes[chunk_start + 4..crc_start] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0_u32.wrapping_sub(crc & 1)));
        }
    }
    bytes[crc_start..crc_start + 4].copy_from_slice(&(!crc).to_be_bytes());
}

#[test]
fn invalid_subframe_rectangle_is_rejected_without_retained_memory() {
    let resources = DecodeResources::new(1, 1 << 20);
    let mut encoded = partial_rect_previous_apng(2);
    let control_type = encoded
        .windows(4)
        .position(|bytes| bytes == b"fcTL")
        .expect("APNG frame control");
    let chunk_start = control_type - 4;
    let x_offset = control_type + 4 + 12;
    encoded[x_offset..x_offset + 4].copy_from_slice(&2_u32.to_be_bytes());
    update_chunk_crc(&mut encoded, chunk_start);
    assert!(matches!(
        decode_animation(
            &mut Cursor::new(&encoded),
            &limits(resources.clone()),
            &CancellationToken::new(),
        ),
        Err(Error::Malformed(MalformedReason::InvalidEncodedData))
    ));
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}
#[test]
fn apng_zero_play_count_means_infinite() {
    let encoded = partial_rect_previous_apng(0);
    let animation = decode_animation(
        &mut Cursor::new(&encoded),
        &limits(DecodeResources::new(1, 1 << 20)),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(animation.loops(), AnimationLoops::Infinite);
    assert_eq!(
        animation
            .frame_at(Duration::from_millis(50))
            .image()
            .pixels(),
        animation.frames()[0].image().pixels()
    );
    assert_eq!(
        animation
            .frame_at(Duration::from_millis(60))
            .image()
            .pixels(),
        animation.frames()[1].image().pixels()
    );
}

#[test]
fn excessive_declared_frame_count_is_rejected_before_canvas_allocation() {
    let resources = DecodeResources::new(1, 1 << 20);
    let mut encoded = partial_rect_previous_apng(2);
    let animation_type = encoded
        .windows(4)
        .position(|bytes| bytes == b"acTL")
        .expect("APNG animation control");
    let chunk_start = animation_type - 4;
    let frame_count = animation_type + 4;
    encoded[frame_count..frame_count + 4].copy_from_slice(&257_u32.to_be_bytes());
    update_chunk_crc(&mut encoded, chunk_start);
    assert!(matches!(
        decode_animation(
            &mut Cursor::new(&encoded),
            &limits(resources.clone()),
            &CancellationToken::new(),
        ),
        Err(Error::BudgetExceeded(ascii_core::BudgetKind::WorkUnits))
    ));
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}

#[test]
fn gif_subframes_compose_disposal_transparency_and_repeat_timeline() {
    let resources = DecodeResources::new(1, 1 << 20);
    let encoded = partial_disposal_gif(Some(gif::Repeat::Finite(1)));
    println!(
        "GIF config_sha256={:x} settings={GIF_SETTINGS}",
        Sha256::digest(GIF_SETTINGS.as_bytes())
    );
    println!("GIF fixture_sha256={:x}", Sha256::digest(&encoded));
    assert_sha256(
        GIF_SETTINGS.as_bytes(),
        "dc797df0c1e556719cea89374be0017617b45ac9abb97cd7acb4ca71f65b1092",
    );
    assert_sha256(
        &encoded,
        "d5a5dc5f85f99530aab8862e84a9c5b4c8536dec155cda61bc1bb013edf8be29",
    );
    let animation = decode_animation(
        &mut Cursor::new(&encoded),
        &limits(resources.clone()),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(animation.frames().len(), 4);
    assert_eq!(animation.loops(), AnimationLoops::Finite(2));
    assert_eq!(animation.loop_duration(), Duration::from_millis(50));
    assert_eq!(animation.frames()[0].duration(), Duration::from_millis(10));
    assert_eq!(animation.frames()[1].duration(), Duration::from_millis(10));
    assert_eq!(animation.frames()[2].duration(), Duration::from_millis(20));
    assert_eq!(animation.frames()[3].duration(), Duration::from_millis(10));
    assert_eq!(
        animation.frames()[0].image().pixels(),
        &[255, 0, 0, 255, 255, 0, 0, 255]
    );
    assert_eq!(
        animation.frames()[1].image().pixels(),
        &[255, 0, 0, 255, 0, 255, 0, 255]
    );
    assert_eq!(
        animation.frames()[2].image().pixels(),
        &[255, 0, 0, 255, 0, 0, 255, 255]
    );
    assert_eq!(
        animation.frames()[3].image().pixels(),
        &[255, 0, 0, 255, 0, 0, 0, 255]
    );
    assert_eq!(
        animation
            .frame_at(Duration::from_millis(50))
            .image()
            .pixels(),
        animation.frames()[0].image().pixels()
    );
    assert_eq!(
        animation
            .frame_at(Duration::from_millis(60))
            .image()
            .pixels(),
        animation.frames()[1].image().pixels()
    );
    assert_eq!(
        animation
            .frame_at(Duration::from_millis(100))
            .image()
            .pixels(),
        animation.frames()[3].image().pixels()
    );
    assert!(resources.cpu_bytes_in_use() > 0);
    drop(animation);
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}

#[test]
fn gif_exact_budget_does_not_double_charge_the_encoded_snapshot() {
    let encoded = partial_disposal_gif(Some(gif::Repeat::Finite(1)));
    let encoded_bytes = u64::try_from(encoded.len()).unwrap();
    let canvas_pixels = 2_u64;
    let canvas_bytes = canvas_pixels * 4;
    let workspace_bytes = canvas_bytes * 3 + canvas_pixels + 64 * 1024;
    let retained_frame_bytes = canvas_bytes * 4;
    let raster_budget = encoded_bytes + workspace_bytes + retained_frame_bytes;
    let resources = DecodeResources::new(1, raster_budget);
    let limits = DecodeLimits::new(
        Budget::new(1 << 20, raster_budget, 1 << 20),
        resources.clone(),
        8,
        8,
        encoded_bytes,
        workspace_bytes,
    );

    let animation = decode_animation(
        &mut Cursor::new(&encoded),
        &limits,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(animation.frames().len(), 4);
    drop(animation);
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}

#[test]
fn gif_infinite_repeat_and_static_still_gate_are_truthful() {
    let resources = DecodeResources::new(1, 1 << 20);
    let encoded = partial_disposal_gif(Some(gif::Repeat::Infinite));
    let animation = decode_animation(
        &mut Cursor::new(&encoded),
        &limits(resources.clone()),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(animation.loops(), AnimationLoops::Infinite);
    drop(animation);
    assert!(matches!(
        decode_image(
            &mut Cursor::new(&encoded),
            &limits(resources.clone()),
            &CancellationToken::new(),
        ),
        Err(Error::Unsupported(UnsupportedReason::UnsupportedFormat))
    ));
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}

#[test]
fn invalid_gif_rectangle_releases_all_decode_resources() {
    let resources = DecodeResources::new(1, 1 << 20);
    let mut encoded = partial_disposal_gif(None);
    let image_descriptor = encoded
        .iter()
        .position(|byte| *byte == 0x2c)
        .expect("first GIF image descriptor");
    encoded[image_descriptor + 1..image_descriptor + 3].copy_from_slice(&2_u16.to_le_bytes());
    assert!(matches!(
        decode_animation(
            &mut Cursor::new(&encoded),
            &limits(resources.clone()),
            &CancellationToken::new(),
        ),
        Err(Error::Malformed(MalformedReason::InvalidEncodedData))
    ));
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}

#[test]
fn gif_excessive_frame_count_releases_all_decode_resources() {
    let resources = DecodeResources::new(1, 1 << 20);
    let palette = [0, 0, 0, 255, 0, 0];
    let mut encoded = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut encoded, 1, 1, &palette).unwrap();
        for _ in 0..=ascii_media::MAX_ANIMATION_FRAMES {
            let mut frame = gif::Frame::from_indexed_pixels(1, 1, vec![1], None);
            frame.delay = 1;
            encoder.write_frame(&frame).unwrap();
        }
    }
    assert!(matches!(
        decode_animation(
            &mut Cursor::new(&encoded),
            &limits(resources.clone()),
            &CancellationToken::new(),
        ),
        Err(Error::BudgetExceeded(ascii_core::BudgetKind::WorkUnits))
    ));
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}

#[test]
fn apng_first_previous_disposal_restores_the_initial_canvas() {
    let resources = DecodeResources::new(1, 1 << 20);
    let mut encoded = partial_rect_previous_apng(2);
    let frame_control = encoded
        .windows(4)
        .position(|bytes| bytes == b"fcTL")
        .expect("first APNG frame control");
    let chunk_start = frame_control - 4;
    encoded[frame_control + 4 + 24] = 2;
    update_chunk_crc(&mut encoded, chunk_start);
    let settings = format!("{APNG_SETTINGS};mutation=frame0.dispose=previous;fcTL-crc=updated");
        &encoded,
        "90446459a823e11715c4e128960ece9b97cb820e7876aa92e31e7b7dfa85f5ca",
    );
    assert_sha256(
        &encoded,
        "90446459a823e11715c4e128960ece9b97cb820e7876aa92e31e7b7dfa85f5ca",
    );
    assert_sha256(
        settings.as_bytes(),
        "a17c43ba4d9d5d33e02c1eb270db702f96a3a2ac4a3c8dedee9ae4ddef9644aa",
    );
    println!(
        "APNG first_previous config_sha256={:x} settings={settings}",
        Sha256::digest(settings.as_bytes())
    );
    println!(
        "APNG first_previous fixture_sha256={:x}",
        Sha256::digest(&encoded)
    );
    let animation = decode_animation(
        &mut Cursor::new(&encoded),
        &limits(resources.clone()),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(
        animation.frames()[1].image().pixels(),
        &[0, 0, 0, 0, 255, 0, 0, 128]
    );
    assert_eq!(
        animation.frames()[2].image().pixels(),
        &[0, 0, 0, 0, 0, 255, 0, 255]
    );
    drop(animation);
    assert_eq!(resources.cpu_bytes_in_use(), 0);
    assert_eq!(resources.active_decodes(), 0);
}
