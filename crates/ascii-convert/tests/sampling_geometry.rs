use ascii_convert::{FitMode, FitTransform};
use ascii_core::Error;

#[test]
fn contain_rejects_extreme_unrepresentable_centering_through_public_api() {
    let maximum_exact_f64_integer = 1_u64 << 53;

    assert_eq!(
        FitTransform::new(1, 1, maximum_exact_f64_integer, 1, FitMode::Contain),
        Err(Error::SizeOverflow)
    );
    // A wider rectangle can pass the extent tolerance while its margins still skew.
    assert_eq!(
        FitTransform::new(5, 1, maximum_exact_f64_integer, 1, FitMode::Contain),
        Err(Error::SizeOverflow)
    );
}

#[test]
fn contain_keeps_ordinary_letterbox_margins_equal() {
    let transform = FitTransform::new(1, 1, 100, 1, FitMode::Contain)
        .expect("ordinary centered geometry is representable");
    let display_rect = transform.display_rect();
    let left_margin = display_rect.x();
    let right_margin = 100.0 - (display_rect.x() + display_rect.width());

    assert_eq!(display_rect.width(), 1.0);
    assert_eq!(left_margin, 49.5);
    assert_eq!(right_margin, 49.5);
}
