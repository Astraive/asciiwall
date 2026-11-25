use std::{io::Cursor, time::Duration};

use ascii_core::{Budget, CancellationToken, Error, MalformedReason, UnsupportedReason};
use ascii_media::{AnimationLoops, DecodeLimits, DecodeResources, decode_animation, decode_image};
use sha2::{Digest, Sha256};

const APNG_SETTINGS: &str = "png=0.18.0;canvas=2x1;frames=5;sample=rgba8;frame0=full:[10,20,30,255,0,0,0,0],dispose=none,blend=source,delay=0/100;frame1=rect(1,0,1,1):[255,0,0,128],dispose=previous,blend=over,delay=1/100;frame2=rect(1,0,1,1):[0,255,0,255],dispose=none,blend=source,delay=1/100;frame3=rect(0,0,1,1):[255,255,0,255],dispose=background,blend=source,delay=1/100;frame4=rect(0,0,1,1):[0,0,255,128],dispose=none,blend=over,delay=1/100;plays=2";

fn limits(resources: DecodeResources) -> DecodeLimits {
    DecodeLimits::new(Budget::new(1 << 20, 1 << 20, 1 << 20), resources, 8, 8, 1 << 20, 1 << 20)
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
        writer
            .set_dispose_op(png::DisposeOp::None)
            .unwrap();
        writer.set_blend_op(png::BlendOp::Source).unwrap();
        writer
            .write_image_data(&[10, 20, 30, 255, 0, 0, 0, 0])
            .unwrap();

        writer.set_frame_dimension(1, 1).unwrap();
        writer.set_frame_position(1, 0).unwrap();
        writer.set_frame_delay(1, 100).unwrap();
        writer
            .set_dispose_op(png::DisposeOp::Previous)
            .unwrap();
        writer.set_blend_op(png::BlendOp::Over).unwrap();
        writer.write_image_data(&[255, 0, 0, 128]).unwrap();

        writer.reset_frame_position().unwrap();
        writer.set_frame_dimension(1, 1).unwrap();
        writer.set_frame_position(1, 0).unwrap();
        writer.set_frame_delay(1, 100).unwrap();
        writer
            .set_dispose_op(png::DisposeOp::None)
            .unwrap();
        writer.set_blend_op(png::BlendOp::Source).unwrap();
        writer.write_image_data(&[0, 255, 0, 255]).unwrap();

        writer.reset_frame_position().unwrap();
        writer.set_frame_dimension(1, 1).unwrap();
        writer.set_frame_position(0, 0).unwrap();
        writer.set_frame_delay(1, 100).unwrap();
        writer
            .set_dispose_op(png::DisposeOp::Background)
            .unwrap();
        writer.set_blend_op(png::BlendOp::Source).unwrap();
        writer.write_image_data(&[255, 255, 0, 255]).unwrap();

        writer.reset_frame_position().unwrap();
        writer.set_frame_dimension(1, 1).unwrap();
        writer.set_frame_position(0, 0).unwrap();
        writer.set_frame_delay(1, 100).unwrap();
        writer
            .set_dispose_op(png::DisposeOp::None)
            .unwrap();
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
    println!("APNG config_sha256={:x} settings={APNG_SETTINGS}", Sha256::digest(APNG_SETTINGS.as_bytes()));
    println!("APNG fixture_sha256={:x}", Sha256::digest(&encoded));

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
        &[10, 20, 30, 255, 0, 0, 0, 0]
    );
    assert_eq!(
        animation.frames()[1].image().pixels(),
        &[10, 20, 30, 255, 255, 0, 0, 128]
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
        &[0, 0, 255, 128, 0, 255, 0, 255]
    );
    assert_eq!(
        animation.frame_at(Duration::ZERO).image().pixels(),
        animation.frames()[0].image().pixels()
    );
    assert_eq!(
        animation.frame_at(Duration::from_millis(10)).image().pixels(),
        animation.frames()[1].image().pixels()
    );
    assert_eq!(
        animation.frame_at(Duration::from_millis(20)).image().pixels(),
        animation.frames()[2].image().pixels()
    );
    assert_eq!(
        animation.frame_at(Duration::from_millis(40)).image().pixels(),
        animation.frames()[4].image().pixels()
    );
    assert_eq!(
        animation.frame_at(Duration::from_millis(100)).image().pixels(),
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
        animation.frame_at(Duration::from_millis(50)).image().pixels(),
        animation.frames()[0].image().pixels()
    );
    assert_eq!(
        animation.frame_at(Duration::from_millis(60)).image().pixels(),
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
