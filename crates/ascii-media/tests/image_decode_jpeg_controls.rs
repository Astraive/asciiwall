//! JPEG decoder controls: progressive scans, EXIF display orientation, in-flight cancellation,
//! and decoder/raster budget preflight.
//!
//! - The ICC rejection variant is derived from the rights-cleared baseline fixture by inserting
//!   one APP2 `ICC_PROFILE\0` chunk (sequence 1 of 1) with a locally generated 128-byte ICC v4.3
//!   monitor-class grayscale/XYZ header; no external media is used.
//!   Complete derived JPEG SHA-256: 253e58d9e5253ad0696ab3f4c765e4518e0f82cbfdeb994aafadd42e6b1adbeb
//! - 4x3 baseline and progressive fixtures use grayscale samples
//!   [20,70,120,170,35,85,135,185,50,100,150,200], quality=95, subsampling=0,
//!   optimize=false; only progressive=false/true differs.
//! - 64x64 cancellation fixture uses Python random.Random(0xA51C).randrange(256)
//!   row-major grayscale samples, quality=90, subsampling=0, optimize=false.
//!   SHA-256 of complete encoded bytes:
//!   baseline 4x3:     90f84b4bf806830d877894c41bc6ab5b0688565237094060abeca4b2fdee4982
//!   progressive 4x3:  ee333a42b8603c25f03293d526ca17352b32b6ad7eaf48e04f77f019f7678581
//!   cancel 64x64:     12870a1e5c0dcfa734a7f00bebb45146696482e158c459afc146d4b0d526166c
//!   The orientation cases prepend a single APP1 segment containing Exif\0\0 and a
//!   little-endian TIFF IFD0 Orientation (0x0112, SHORT, count 1); complete variant hashes:
//!   1 ea6cc82b7cf4b418300eeb827e9ca22157cdd1134bdb2a2a73ee6d842d3eefec
//!   2 c776304658b573bfcb2b906be15b02731a0649efb56bdd81274ff62fed42b7b7
//!   3 a06d5ab8f0c27594a1e7fca500ecb19412256ff19d6d770afcad4de320623258
//!   4 90fb9b9b9ee9e96ca91b6eed1fae825dfe426a23fcc77ce2dadeace2522451e8
//!   5 4629a21cd0cc45fe8fc594be0e40fa761c2536f92caac6906b3336785c92bd75
//!   6 5bc7485e75366ad9e771708914faa9f59a81a27865268d2f1c64fe9d6a997f08
//!   7 0244a3ecd7fcf76d8627c5522f1f770322fa600276577898733754b7336fd979
//!   8 73a3b4254d1b5cdb940fba0f8f6b82fea960b29c3114d42c7433adc418a8481f

use std::io::{self, Cursor, Read, Seek, SeekFrom};

use ascii_core::{
    Budget, BudgetKind, CancellationToken, ColorRange, Error, MalformedReason, SourceProfile,
    UnsupportedReason,
use ascii_media::{DecodeLimits, DecodeResources, DecodedFrame, ReadSeek, decode_image};
use sha2::{Digest, Sha256};

const BASELINE_JPEG_HEX: &str = concat!(
    "ffd8ffe000104a46494600010100000100010000ffdb004300020101010101020101010202020202",
    "0403020202020504040304060506060605060606070908060709070606080b08090a0a0a0a0a0608",
    "0b0c0b0a0c090a0a0affc0000b080003000401011100ffc4001f0000010501010101010100000000",
    "000000000102030405060708090a0bffc400b5100002010303020403050504040000017d01020300",
    "041105122131410613516107227114328191a1082342b1c11552d1f02433627282090a161718191a",
    "25262728292a3435363738393a434445464748494a535455565758595a636465666768696a737475",
    "767778797a838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9ba",
    "c2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1f2f3f4f5f6f7f8f9faffda",
    "0008010100003f00f74ff82497813c21ff000c41e17ff8905bff001763fdc8fdebffd9",
);

const PROGRESSIVE_JPEG_HEX: &str = concat!(
    "ffd8ffe000104a46494600010100000100010000ffdb004300020101010101020101010202020202",
    "0403020202020504040304060506060605060606070908060709070606080b08090a0a0a0a0a0608",
    "0b0c0b0a0c090a0a0affc2000b080003000401011100ffc400140001000000000000000000000000",
    "00000006ffda000801010000000175ffc400161001010100000000000000000000000000040716ff",
    "da00080101000105029204988fffc4001910000105000000000000000000000000000002041123f0",
    "ffda0008010100063f026b42741fffc4001610000300000000000000000000000000000071b1ffda",
    "0008010100013f216ea0ffda00080101000000107fffc40016100101010000000000000000000000",
    "0000b10001ffda0008010100013f103d35afffd9",
);

const CANCEL_JPEG_HEX: &str = concat!(
    "ffd8ffe000104a46494600010100000100010000ffdb004300030202030202030303030403030405",
    "0805050404050a070706080c0a0c0c0b0a0b0b0d0e12100d0e110e0b0b1016101113141515150c0f",
    "171816141812141514ffc0000b080040004001011100ffc4001f0000010501010101010100000000",
    "000000000102030405060708090a0bffc400b5100002010303020403050504040000017d01020300",
    "041105122131410613516107227114328191a1082342b1c11552d1f02433627282090a161718191a",
    "25262728292a3435363738393a434445464748494a535455565758595a636465666768696a737475",
    "767778797a838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9ba",
    "c2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1f2f3f4f5f6f7f8f9faffda",
    "0008010100003f00e97c39a45edff8535059f56d47c3a9a3dc49a8ebb61e28d3676b4944d225d3d9",
    "5e5a5d246925a43009645649d250be71fb2b48f86e77c612d96bd66bace9fade8fa4eb57be226b63",
    "adcfb62bf9ad3ecd676f0df45beec3b00f6cb731457370d3cd3db2322855962aad6977e2fb0f0ec3",
    "f65f156947419f438adc5ccb6e2eedac636baba92c2392dafccaf3978e5d3d4c6b893cb92c9be748",
    "94dcf3376fe1ad43e145bff61cf690e996733decd79e1b8a5962beb52817fb5d6cad9e04b3b891ed",
    "e58cdbc8e86348a48a109feb0f57e13f0cea7e22d0b45b0f07b7882eadbfe124b78e089a1bdb1b9d",
    "3de45b632b5bca5b65a21b14deac5e6902348a7ed065172aef8717d3cda5784fc5ba6ff63c715f47",
    "15dfda74a823d32c2e821b9967b4b88fcc8a288c908b3592346307da07ef95d9d19343e14cda27c4",
    "2bab195bc5323f8462551aa32d8fd934ad062d3a08596da7fb5a4ac1046f1c6023c71c71c9236eb8",
    "7372c27f05e9f67e1d6b9f0fc777a0e9d636b6769756da87f672e9f3c77100b67b4893ed73cd1422",
    "47476fb9349242d911ba4d1309b54f0cebfe2fd3b53d1ffb5f41b3d1f5f9668748b2bdb1588d8dd5",
    "b4df636b396eee1965b9437373079e445299d832a0324b23c793e12d5ec60d79a666bcd274fd3e68",
    "6eb48d559249e39bccd4a599ee2dee5911ee2e3ec9746658d59ee9d1ae96762c9e59e3f521a7d8e8",
    "9e259f56bdd2c597882cbfb6351f0a2ea02e6e65b66b9405664cdb190dbbcda9a848a5c4620891da",
    "39218eea1b1f14b4ff000f6a76be26822beb6d1352bdb46b63add9ea32dd43aa6a36b05842f2490a",
    "c3732c6522ba95bca8a247892da42b222ccde5f6be21f10ebbe1cd52253e29d42d345b137d7f677b",
    "6d6773a8a8b89edae61ba74599c4d05cdb2a5e06b49364d0bcd34d2cd2c314c165d7bc0ba6f863c1",
    "7a0d8e9fafe9ba3e85a3dcdb5e49af5fbdda36a3a6c70caef0ceed0cf031f3ee01922c98cadfc91b",
    "aefcbb77de39bbd7fc5de13b8b8f08eb5f68b4d3ed1ee6fb42cbc57115cb481ed61308f25a34484b",
    "bab82be7170e0242b0cb07855baeba96be336f0a47e66a9af497576fa179f6d243144c4c50cf2461",
    "e7962b496defafae0dc34f0ae248d4958bc9697762d6be22e9ad79e1fd3f48b53e05b1bd5b0bbf10",
    "78a26b88e4b4b76b8fb459dca5c7da71e6c10db34ced1cb0217b90e06d58e63ada1f86bc5a74b8e5",
    "5f14f843518ad0ff00605a5b5b44f1c6ba7dd5d4f14335cc29e4ccca66092432075d81d9d5940679",
    "eabfda7c1169adf8d668b4ad1f48f1ae8336a571a95ae9d8b0b0d325b28e1b0b033cc8c612034523",
    "431c0d23b0b28d494049cdf0a1d63c45a5695aae93a068f36bd6bab3ff0065788adafaf1e1d52133",
    "4b343a7a46bf3388ee6486648e4926089672b3c481543def03dd7882c535cd1a7bbd334bb18956ea",
    "2d1ef2c248afee0c3669037da63b696487ca78d1634b6569f31ddc1e6ef324131d0f859ff092da78",
    "b2e65b1d7b5dd6df59b2d6a1b2f0eeaf349656bf6acdbdcaa18a59e1b8fb464426500bac5f6c0e26",
    "3e4dc91cd6a5e3dd1f53d522d12ecc5a241a66950cb617512fd86d278e1b7451a389a28ade4f28a4",
    "a6499a38e2653a94adb218a19239e9f83d357f87fe29b99a1f0a4916a36fe23b4b9782058af24d36",
    "28525f263ba115d3acf2491dc46239922603cf8d621e6cf6de5749e24f11a4be0df12eab65e3ef06",
    "6af726ca7b9b55d32efecb7c21596d1ad5e4bd324b379933d9033ac8e9ba69113cd2c2171c7b681e",
    "3cf125d696fe2cd0cc3716be2083c45fd9e634d267935596e01b9b24bef3952456b48d4b48ce65b7",
    "103c4ca190b5761aef8727d43c6bae6997eca9e1cd3f488afac65d5ede39bc8fb369d68f2cb1a5d4",
    "52e2168fed88f35ca49879623b669a368df82bdf11cffdb96fe2793c602ee092cd3c452689ae3c37",
    "306a1a8411a13332324ab190acd00292c8f198163431b7930c1eaba0be83a9e93a0476ab79a969f6",
    "f7a0c9aceb51c77f05ad8bde5ccf0bef1091e4674a698c8a1a499e5025d80ef1c0697e39bad3fc61",
    "7da15aeab1f95e1cb88449a6e9f72e2da48a44699ac41b551670c6c89319226fb4ab0b281122ba74",
    "7ddd678a357bcbdf166a97a6ee352d7d8fb178aed2dbecb631fd85ee14497cc8cf6db2f26b84485d",
    "5e5616ef1abef8dd61e3358835cb7d2e4d021f106b4f77697f2f922faf06ad63656b0cd6f791dccd",
    "7ae630b1a3ca37ec08accd68c1ddedd21373c3935e6a5a768f269eda66b3a5cc1b4db4f0cd9416f2",
    "3dce9772d2451e2eedd64b8bc7852531b22aca5248aecbb850fbb4b43d43c4bf11744d0f4bf0ef86",
    "3c30d26916375a5dc59e8b6f1dbc52cf741d45acb6fb91cc71a5f5ef9e648e04575bb60ac6100dad",
    "53e286b1e37f095ddce9cde22b6d71c4ba85bdd5c5b90f2594d0c11ea33a4048f26346bfb99066e0",
    "244f6d6e84398d7671b06a1a56ba2d7c536f76faa784ec645b6b284e857eb158d87956924f2848ae",
    "5d6385d835bdccf109d61f2c49e487b801fb28fc633cde29f09f8bb44d2aea686e14dd4492dd2acd",
    "ab886ded9f6dbdc4d28ba2c6dae951425caef36455adc44cc91e5687e13d6f49d6f56f0958ea7a8e",
    "b17ff64d4e0d5ed357b80d6315988a48208558dc6c88e21b6b0bbda5480f0792c57124d7bc1fe21f",
    "1249a84720d42df4bf0df88350874bd3efaeed1353d42ca4bcd2a3927b9927b9811ae22583ccdf31",
    "9c46e91830a3448a8618ace4f1a5e417baaa4573a25d5b5fe8115ac7a449672f87ef63f326de2c19",
    "2592e23852eda0b7590c10c5e54714bb7e52dd8f8f0f82edbc430ea13cfaaddf8aefe6b7d4f4ad5a",
    "dbedf04d2e6eaea3b3b66678840f23319625dcb6e638c6d1910a97e364f106b9e1ed3ae343693523",
    "a035d49a49b9b9813568044628ac96799639e55f98e98d3ca821984d2492aa49bc3bae9fc1abef12",
    "7c44f095ce84759935289b49cea563776e356b4bd12fda95ec26bfb59dde196781e16f3e380b3c71",
    "409b1a55c4904b1f88fc6ff0e935cb8bebdd77c24c64bb4b7b37b08a4967b2b310dd5dc56e96e04b",
    "1db4b676ca9b51645612b22491ff00ac9f447d0f47f195a1d62cff00b1ef64177a96adada695773d",
    "b9d362b385e58d668e0b39ed9e38af12316c196180c00b09ae9807a9a45b2687ac7866d755b7b5d3",
    "7c4fad6b02054d275399de2b46b9bc79ef219634905ccdb2c8a7da447f6a47861324aef2b2c78ea2",
    "2f0a584171a4784214d3d1ed2d5acb4bd2d2c60749f3018a6b379af11e431c76d33cd712c3b5a4b2",
    "f30a34bbc75f178461f0078bcf8d74ebaf09deda5d9bdb5d36feea27b9964d356e0dbdcddc9756ea",
    "d0f9e92cb108985c44521892207264da9e305d6fc25a0c1e11d63c3ba3eafe1cb64b6b87d423d155",
    "6466b785226b85623f7cd0adb991acd4cb11b7fb4a35d476c6255f3c9b5fb9f887aacde1df138b8b",
    "668a2b54be6d4aead6c534d7b233895a1b31e6857fb75cc76df6a925f324b89a65559504d047ea70",
    "2e9d1f80344d255a1d43c3fa5c17da4db5ede9b46d460991043a6dd4524f32db936cf77341bd495f",
    "36ebe4f30ab48b4fc45e30d7bc14b0786357bab1f1c788f504df70963676f1da5f986e6de6b69a04",
    "86d91e3731fdb36973e5a5c48811da34936e3c1e25d3fc4baff8b1e1d2d2ef447561ac689a4482c1",
    "ef2499e3be8b528614921920f36c919143bb282b69e634c6295e3e9bc59e1bf0d407c7fe20b6f075",
    "adb477ba9de410c7aadacd631c9750132db994dbfcf782e6e6d42ac31486348c5bc8b14466cb731a",
    "5787fc53e18d5b4ad1acb5bd57c45a6c16773a9dc5c69af3697a3c6b246b71a92c4f1d94915c0962",
    "ba93c99e24b75b5374ebe602d1905969175e2cf07780f4ad216e6cce9d684dade788617d3a6b2bd6",
    "bc4bc522d363dadc2a3da42165190c97108f2a66458a593e1edaeb371e1bd1ad3c2d2eb5e104d366",
    "965b2d0b4b9ada38f53b82d7714d725609239658b6daceeb044f2c6cb142a673e64862cdf86df0fb",
    "c37e2bb1d5fc116de12d4355d3a58468baa6896d6b245771ddee81e2592e27bb778cc51db4225454",
    "f2d242a25da44461d997e147877e27f8d3c407c3a9e29d42d35096e74b89bc2d6f1298a1bab5f245",
    "d05b88ade2f9ed603e610cd1a2dcc4912a6fdd0d0f125a0d4124d5adb52b6f0bdaeb22d85bdbdaf8",
    "923b6b792cede05d93a5d6cc4225d3ef2e6308414c5a912caf3150b565f156b5e209bc4de29f11ea",
    "96d63ab69fa65fc7ae693732417cb657b1dda14f2a531621b667f315e375b9f360912da3fb4e6148",
    "75be2a42351bf36daa69f149a858ea569aa58c3ac5e3ea91b5fe6281902c16f71706cae1a28a4282",
    "2844cc90812b4926d8f73c152f8d2ebe23787ae24d435ad4753ba6b14d46ebc496d1bda9b2be8add",
    "479aa9046cdf6bb7b6702dfcd95239ed1559a79a78e513f8bee107c30f0c6a7ac6a36da5c3a5add6",
    "97a06a73e8f70e6d5d516d6d2196396dee25995d6eae1da36591a59ed232a40f3113ffd9",
);

const SOURCE_SAMPLES: &[u8] = &[20, 70, 120, 170, 35, 85, 135, 185, 50, 100, 150, 200];

const ORIENTATION_OUTPUTS: [((u32, u32), &[u8]); 8] = [
    (
        (4, 3),
        &[20, 70, 120, 170, 35, 85, 135, 185, 50, 100, 150, 200],
    ),
    (
        (4, 3),
        &[170, 120, 70, 20, 185, 135, 85, 35, 200, 150, 100, 50],
    ),
    (
        (4, 3),
        &[200, 150, 100, 50, 185, 135, 85, 35, 170, 120, 70, 20],
    ),
    (
        (4, 3),
        &[50, 100, 150, 200, 35, 85, 135, 185, 20, 70, 120, 170],
    ),
    (
        (3, 4),
        &[20, 35, 50, 70, 85, 100, 120, 135, 150, 170, 185, 200],
    ),
    (
        (3, 4),
        &[50, 35, 20, 100, 85, 70, 150, 135, 120, 200, 185, 170],
    ),
    (
        (3, 4),
        &[200, 185, 170, 150, 135, 120, 100, 85, 70, 50, 35, 20],
    ),
    (
        (3, 4),
        &[170, 185, 200, 120, 135, 150, 70, 85, 100, 20, 35, 50],
    ),
];

fn limits() -> DecodeLimits {
    DecodeLimits::new(
        Budget::new(1 << 20, 1 << 20, 1 << 20),
        DecodeResources::new(4, 16 << 20),
        1024,
        1024,
        1 << 20,
        1 << 20,
    )
}

fn fixture_bytes(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
        .collect()
}

fn assert_fixture_sha256(bytes: &[u8], expected: &str) {
    let actual = format!("{:x}", Sha256::digest(bytes));
    println!("jpeg fixture sha256={actual}");
    assert_eq!(actual, expected);
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => panic!("invalid fixture hex digit"),
    }
}

fn decode_with(
    bytes: &[u8],
    decode_limits: &DecodeLimits,
    cancellation: &CancellationToken,
) -> Result<DecodedFrame, Error> {
    let mut input = Cursor::new(bytes);
    decode_image(&mut input as &mut dyn ReadSeek, decode_limits, cancellation)
}

fn decode(bytes: &[u8]) -> Result<DecodedFrame, Error> {
    decode_with(bytes, &limits(), &CancellationToken::new())
}

fn assert_gray_grid(frame: &DecodedFrame, dimensions: (u32, u32), expected: &[u8]) {
    let (width, height) = dimensions;
    assert_eq!((frame.width(), frame.height()), dimensions);
    assert_eq!(frame.row_stride(), width as usize * 3);
    assert_eq!(frame.pixels().len(), expected.len() * 3);
    assert_eq!(expected.len(), (width * height) as usize);

    for (index, (pixel, expected)) in frame
        .pixels()
        .as_chunks::<3>()
        .0
        .iter()
        .zip(expected.iter().copied())
        .enumerate()
    {
        for (channel_index, channel) in pixel.iter().copied().enumerate() {
            assert!(
                channel.abs_diff(expected) <= 8,
                "pixel {index}, channel {channel_index}: expected gray {expected}, got {channel}"
            );
        }
        assert!(pixel[0].abs_diff(pixel[1]) <= 3);
        assert!(pixel[1].abs_diff(pixel[2]) <= 3);
    }
}

fn with_exif_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
    assert!((1..=8).contains(&orientation));
    with_exif_orientation_entry(jpeg, 3, 1, orientation)
}

fn with_exif_orientation_entry(
    jpeg: &[u8],
    value_type: u16,
    value_count: u32,
    orientation: u16,
) -> Vec<u8> {
    assert_eq!(&jpeg[..2], &[0xff, 0xd8]);

    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&[
        b'I', b'I', 42, 0, 8, 0, 0, 0, // TIFF header; IFD0 starts at offset 8.
        1, 0, // One IFD entry.
        0x12, 0x01, // Orientation tag.
    ]);
    payload.extend_from_slice(&value_type.to_le_bytes());
    payload.extend_from_slice(&value_count.to_le_bytes());
    payload.extend_from_slice(&orientation.to_le_bytes());
    payload.extend_from_slice(&[0; 6]); // Inline value padding and next-IFD offset.

    let app1_length = u16::try_from(payload.len() + 2).expect("small EXIF segment");
    let mut tagged = Vec::with_capacity(jpeg.len() + payload.len() + 4);
    tagged.extend_from_slice(&jpeg[..2]);
    tagged.extend_from_slice(&[0xff, 0xe1]);
    tagged.extend_from_slice(&app1_length.to_be_bytes());
    tagged.extend_from_slice(&payload);
    tagged.extend_from_slice(&jpeg[2..]);
    tagged
}

fn with_exif_ifd_pointer(jpeg: &[u8], exif_ifd_offset: u32) -> Vec<u8> {
    assert_eq!(&jpeg[..2], &[0xff, 0xd8]);

    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&[
        b'I', b'I', 42, 0, 8, 0, 0, 0, // TIFF header; IFD0 starts at offset 8.
        1, 0, // One IFD entry.
        0x69, 0x87, 4, 0, 1, 0, 0, 0, // Exif IFD pointer, LONG, one value.
    ]);
    payload.extend_from_slice(&exif_ifd_offset.to_le_bytes());
    payload.extend_from_slice(&[0; 4]); // Next-IFD offset.

    let app1_length = u16::try_from(payload.len() + 2).expect("small EXIF segment");
    let mut tagged = Vec::with_capacity(jpeg.len() + payload.len() + 4);
    tagged.extend_from_slice(&jpeg[..2]);
    tagged.extend_from_slice(&[0xff, 0xe1]);
    tagged.extend_from_slice(&app1_length.to_be_bytes());
    tagged.extend_from_slice(&payload);
    tagged.extend_from_slice(&jpeg[2..]);
    tagged
}

fn with_exif_color_space(jpeg: &[u8], color_space: u16) -> Vec<u8> {
    assert_eq!(&jpeg[..2], &[0xff, 0xd8]);

    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&[
        b'I', b'I', 42, 0, 8, 0, 0, 0, // TIFF header; IFD0 starts at offset 8.
        1, 0, // One IFD0 entry.
        0x69, 0x87, 4, 0, 1, 0, 0, 0, 26, 0, 0, 0, // Exif IFD pointer.
        0, 0, 0, 0, // Next IFD offset.
        1, 0, // One Exif IFD entry.
        1, 0xa0, 3, 0, 1, 0, 0, 0, // ColorSpace tag, SHORT, one value.
    ]);
    payload.extend_from_slice(&color_space.to_le_bytes());
    payload.extend_from_slice(&[0; 2]); // Inline value padding.
    payload.extend_from_slice(&[0; 4]); // Next IFD offset.

    let app1_length = u16::try_from(payload.len() + 2).expect("small EXIF segment");
    let mut tagged = Vec::with_capacity(jpeg.len() + payload.len() + 4);
    tagged.extend_from_slice(&jpeg[..2]);
    tagged.extend_from_slice(&[0xff, 0xe1]);
    tagged.extend_from_slice(&app1_length.to_be_bytes());
    tagged.extend_from_slice(&payload);
    tagged.extend_from_slice(&jpeg[2..]);
    tagged
}

fn with_comment_padding(jpeg: &[u8], padding_len: usize) -> Vec<u8> {
    assert_eq!(&jpeg[..2], &[0xff, 0xd8]);

    let mut padded = Vec::with_capacity(jpeg.len() + padding_len + 4);
    padded.extend_from_slice(&jpeg[..2]);
    let mut remaining = padding_len;
    while remaining > 0 {
        let chunk_len = remaining.min(65_533);
        let comment_length = u16::try_from(chunk_len + 2).expect("small comment segment");
        padded.extend_from_slice(&[0xff, 0xfe]);
        padded.extend_from_slice(&comment_length.to_be_bytes());
        padded.resize(padded.len() + chunk_len, 0);
        remaining -= chunk_len;
    }
    padded.extend_from_slice(&jpeg[2..]);
    padded
}

fn with_icc_profile(jpeg: &[u8]) -> Vec<u8> {
    assert_eq!(&jpeg[..2], &[0xff, 0xd8]);

    let mut profile = [0_u8; 128];
    profile[0..4].copy_from_slice(&128_u32.to_be_bytes());
    profile[4..8].copy_from_slice(b"rust");
    profile[8..12].copy_from_slice(&0x0430_0000_u32.to_be_bytes());
    profile[12..16].copy_from_slice(b"mntr");
    profile[16..20].copy_from_slice(b"GRAY");
    profile[20..24].copy_from_slice(b"XYZ ");
    profile[24..36].copy_from_slice(&[0x07, 0xe8, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    profile[36..40].copy_from_slice(b"acsp");
    profile[40..44].copy_from_slice(b"APPL");
    profile[64..68].copy_from_slice(&0_u32.to_be_bytes());
    profile[68..72].copy_from_slice(&0x0000_f6d6_u32.to_be_bytes());
    profile[72..76].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
    profile[76..80].copy_from_slice(&0x0000_d32d_u32.to_be_bytes());
    profile[80..84].copy_from_slice(b"asci");

    let mut payload = b"ICC_PROFILE\0".to_vec();
    payload.extend_from_slice(&[1, 1]);
    payload.extend_from_slice(&profile);
    let app2_length = u16::try_from(payload.len() + 2).expect("small ICC segment");

    let mut tagged = Vec::with_capacity(jpeg.len() + payload.len() + 4);
    tagged.extend_from_slice(&jpeg[..2]);
    tagged.extend_from_slice(&[0xff, 0xe2]);
    tagged.extend_from_slice(&app2_length.to_be_bytes());
    tagged.extend_from_slice(&payload);
    tagged.extend_from_slice(&jpeg[2..]);
    tagged
}

fn with_full_255_chunk_icc_profile(jpeg: &[u8]) -> Vec<u8> {
    assert_eq!(&jpeg[..2], &[0xff, 0xd8]);

    let mut profile = [0_u8; 255];
    profile[0..4].copy_from_slice(&255_u32.to_be_bytes());
    profile[4..8].copy_from_slice(b"rust");
    profile[8..12].copy_from_slice(&0x0430_0000_u32.to_be_bytes());
    profile[12..16].copy_from_slice(b"mntr");
    profile[16..20].copy_from_slice(b"GRAY");
    profile[20..24].copy_from_slice(b"XYZ ");
    profile[24..36].copy_from_slice(&[0x07, 0xe8, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    profile[36..40].copy_from_slice(b"acsp");
    profile[40..44].copy_from_slice(b"APPL");
    profile[64..68].copy_from_slice(&0_u32.to_be_bytes());
    profile[68..72].copy_from_slice(&0x0000_f6d6_u32.to_be_bytes());
    profile[72..76].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
    profile[76..80].copy_from_slice(&0x0000_d32d_u32.to_be_bytes());
    profile[80..84].copy_from_slice(b"asci");

    let mut tagged = Vec::with_capacity(jpeg.len() + profile.len() * 19);
    tagged.extend_from_slice(&jpeg[..2]);
    for (index, profile_byte) in profile.iter().copied().enumerate() {
        tagged.extend_from_slice(&[0xff, 0xe2]);
        tagged.extend_from_slice(&17_u16.to_be_bytes());
        tagged.extend_from_slice(b"ICC_PROFILE\0");
        tagged.extend_from_slice(&[
            u8::try_from(index + 1).expect("ICC sequence fits in one byte"),
            u8::MAX,
            profile_byte,
        ]);
    }
    tagged.extend_from_slice(&jpeg[2..]);
    tagged
}

#[test]
fn baseline_and_progressive_jpegs_decode_with_expected_dimensions_and_pixels() {
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    assert!(
        baseline
            .windows(2)
            .any(|marker| marker[0] == 0xff && marker[1] == 0xc0)
    );
    assert!(
        !baseline
            .windows(2)
            .any(|marker| marker[0] == 0xff && marker[1] == 0xc2)
    );
    let baseline_frame = decode(&baseline).unwrap();
    assert_gray_grid(&baseline_frame, (4, 3), SOURCE_SAMPLES);

    let progressive = fixture_bytes(PROGRESSIVE_JPEG_HEX);
    assert!(
        progressive
            .windows(2)
            .any(|marker| marker[0] == 0xff && marker[1] == 0xc2)
    );
    let progressive_frame = decode(&progressive).unwrap();
    assert_gray_grid(&progressive_frame, (4, 3), SOURCE_SAMPLES);
}

#[test]
fn progressive_scan_work_is_budgeted_in_addition_to_pixel_work() {
    let progressive = fixture_bytes(PROGRESSIVE_JPEG_HEX);
    let mut decode_limits = limits();
    let pixel_work = u64::try_from(SOURCE_SAMPLES.len()).unwrap() * 2;
    decode_limits.budget = Budget::new(
        1 << 20,
        1 << 20,
        u64::try_from(progressive.len()).unwrap() + pixel_work + 64,
    );

    assert!(matches!(
        decode_with(&progressive, &decode_limits, &CancellationToken::new()),
        Err(Error::BudgetExceeded(BudgetKind::WorkUnits))
    ));
    assert_eq!(decode_limits.resources.cpu_bytes_in_use(), 0);
    assert_eq!(decode_limits.resources.active_decodes(), 0);
}

#[test]
fn unsupported_jpeg_depth_and_color_model_have_capability_errors() {
    let mut depth_12 = fixture_bytes(BASELINE_JPEG_HEX);
    let sof = depth_12
        .windows(2)
        .position(|marker| marker == [0xff, 0xc0])
        .unwrap();
    depth_12[sof + 4] = 12; // SOF precision, ahead of any pixel decompression.
    assert!(matches!(
        decode(&depth_12),
        Err(Error::Unsupported(UnsupportedReason::SampleDepth))
    ));

    // Rights-cleared synthetic 1x1 CMYK; mozjpeg 0.10.13, quality 95, default settings.
    let mut encoder = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_CMYK);
    encoder.set_size(1, 1);
    encoder.set_quality(95.0);
    let mut started = encoder.start_compress(Vec::new()).unwrap();
    started.write_scanlines(&[0, 64, 128, 255]).unwrap();
    let cmyk = started.finish().unwrap();
    println!("jpeg cmyk fixture sha256={:x}", Sha256::digest(&cmyk));
    assert!(matches!(
        decode(&cmyk),
        Err(Error::Unsupported(UnsupportedReason::PixelFormat))
    ));
}

#[test]
fn exif_srgb_color_space_is_preserved_and_uncalibrated_color_is_rejected() {
    const SETTINGS: &str = "jpeg=rights-cleared-baseline-4x3;exif=II;IFD0.ExifIFD=26;ExifIFD.ColorSpace=SHORT;values=sRGB(1),uncalibrated(65535)";
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    let srgb = with_exif_color_space(&baseline, 1);
    let uncalibrated = with_exif_color_space(&baseline, u16::MAX);
    println!(
        "exif color-space fixture settings_sha256={:x} settings={SETTINGS}",
        Sha256::digest(SETTINGS.as_bytes())
    );
    println!(
        "jpeg exif colorspace=sRGB sha256={:x}",
        Sha256::digest(&srgb)
    );
    println!(
        "jpeg exif colorspace=uncalibrated sha256={:x}",
        Sha256::digest(&uncalibrated)
    );

    let tagged = decode(&srgb).unwrap();
    assert_eq!(tagged.color().source_profile(), SourceProfile::Srgb);
    assert_eq!(tagged.color().range(), ColorRange::Unspecified);
    assert_eq!(tagged.color().alpha_mode(), ascii_core::AlphaMode::Opaque);
    assert!(matches!(
        decode(&uncalibrated),
        Err(Error::Unsupported(UnsupportedReason::ColorDescription))
    ));
}

#[test]
fn exif_without_orientation_uses_identity_transform() {
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    let tagged = with_exif_color_space(&baseline, 1);
    let frame = decode(&tagged).unwrap();
    assert_gray_grid(&frame, (4, 3), SOURCE_SAMPLES);
}

#[test]
fn malformed_exif_orientation_and_ifd_pointer_are_rejected() {
    const SETTINGS: &str = "jpeg=rights-cleared-baseline-4x3;malformed-exif=orientation-type-LONG,orientation-count-2,orientation-value-9,ifd-pointer-u32-max";
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    let fixtures = [
        (
            "orientation-wrong-type",
            with_exif_orientation_entry(&baseline, 4, 1, 1),
        ),
        (
            "orientation-wrong-count",
            with_exif_orientation_entry(&baseline, 3, 2, 1),
        ),
        (
            "orientation-out-of-range",
            with_exif_orientation_entry(&baseline, 3, 1, 9),
        ),
        (
            "invalid-exif-ifd-pointer",
            with_exif_ifd_pointer(&baseline, u32::MAX),
        ),
    ];
    println!(
        "malformed EXIF fixture settings_sha256={:x} settings={SETTINGS}",
        Sha256::digest(SETTINGS.as_bytes())
    );
    for (name, fixture) in fixtures {
        println!("malformed EXIF {name} sha256={:x}", Sha256::digest(&fixture));
        assert!(
            matches!(
                decode(&fixture),
                Err(Error::Malformed(MalformedReason::InvalidEncodedData))
            ),
            "{name} metadata was not rejected as malformed"
        );
    }
}

#[test]
fn all_exif_orientations_transform_coordinates_exactly_once() {
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    for (index, (dimensions, expected)) in ORIENTATION_OUTPUTS.iter().enumerate() {
        let orientation = u16::try_from(index + 1).unwrap();
        let tagged = with_exif_orientation(&baseline, orientation);
        let frame = decode(&tagged).unwrap();
        assert_gray_grid(&frame, *dimensions, expected);
    }
}

#[test]
fn swapped_exif_dimensions_are_checked_after_orientation() {
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    let tagged = with_exif_orientation(&baseline, 6);
    let mut decode_limits = limits();
    decode_limits.max_width = 3;
    decode_limits.max_height = 4;

    let frame = decode_with(&tagged, &decode_limits, &CancellationToken::new()).unwrap();
    assert_gray_grid(&frame, ORIENTATION_OUTPUTS[5].0, ORIENTATION_OUTPUTS[5].1);
}

#[test]
fn jpeg_with_icc_profile_is_rejected_as_unsupported_color_description() {
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    let profiled = with_icc_profile(&baseline);
    assert_fixture_sha256(
        &profiled,
        "253e58d9e5253ad0696ab3f4c765e4518e0f82cbfdeb994aafadd42e6b1adbeb",
    );

    match decode(&profiled) {
        Err(error) => assert_eq!(
            error,
            Error::Unsupported(UnsupportedReason::ColorDescription)
        ),
        Ok(_) => panic!("ICC-profiled JPEG unexpectedly published a frame"),
    }
}

#[test]
fn incomplete_icc_profile_is_not_silently_treated_as_srgb() {
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    let mut profiled = with_icc_profile(&baseline);
    // The only APP2 ICC segment now declares two segments.
    assert_eq!(&profiled[6..18], b"ICC_PROFILE\0");
    profiled[19] = 2;
    assert!(matches!(
        decode(&profiled),
        Err(Error::Unsupported(UnsupportedReason::ColorDescription))
    ));
}

#[test]
fn complete_255_chunk_icc_profile_is_rejected() {
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    let profiled = with_full_255_chunk_icc_profile(&baseline);
    assert_eq!(
        profiled
            .windows(2)
            .filter(|marker| marker[0] == 0xff && marker[1] == 0xe2)
            .count(),
        255
    );
    assert!(matches!(
        decode(&profiled),
        Err(Error::Unsupported(UnsupportedReason::ColorDescription))
    ));
}

#[test]
fn truncated_jpeg_is_reported_as_malformed() {
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    assert!(matches!(
        decode(&baseline[..baseline.len() - 2]),
        Err(Error::Malformed(_))
    ));
}

#[test]
fn jpeg_rejects_an_eoi_marker_in_place_of_entropy_data() {
    // Derived from the hashed 4x3 baseline fixture: retain its entire SOS header,
    // remove the scan entropy, and leave an otherwise well-formed EOI marker.
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    let entropy_start = jpeg_entropy_start(&baseline);
    assert_eq!(&baseline[baseline.len() - 2..], b"\xff\xd9");
    assert!(entropy_start < baseline.len() - 2);
    let mut missing_entropy = baseline[..entropy_start].to_vec();
    missing_entropy.extend_from_slice(b"\xff\xd9");
    println!(
        "empty-entropy JPEG fixture sha256={:x}",
        Sha256::digest(&missing_entropy)
    );

    let decode_limits = limits();
    let token = CancellationToken::new();
    assert!(matches!(
        decode_with(&missing_entropy, &decode_limits, &token),
        Err(Error::Malformed(_))
    ));
    assert_eq!(decode_limits.resources.cpu_bytes_in_use(), 0);
    assert_eq!(decode_limits.resources.active_decodes(), 0);

    let frame = decode_with(&baseline, &decode_limits, &token).unwrap();
    assert_gray_grid(&frame, (4, 3), SOURCE_SAMPLES);
    drop(frame);
    assert_eq!(decode_limits.resources.cpu_bytes_in_use(), 0);
}

#[test]
fn jpeg_rejects_missing_entropy_and_invalid_huffman_codes() {
    // Fully local 8x8 gray JPEG: q0 contains 64 ones; DC and AC Huffman tables
    // each have one one-bit code 0, mapping respectively to category 0 and EOB.
    let mut header = vec![0xff, 0xd8, 0xff, 0xdb, 0, 0x43, 0];
    header.extend_from_slice(&[1; 64]);
    header.extend_from_slice(&[0xff, 0xc0, 0, 11, 8, 0, 8, 0, 8, 1, 1, 0x11, 0]);
    for class in [0_u8, 0x10] {
        header.extend_from_slice(&[0xff, 0xc4, 0, 20, class, 1]);
        header.extend_from_slice(&[0; 15]);
        header.push(0);
    }
    header.extend_from_slice(&[0xff, 0xda, 0, 8, 1, 1, 0, 0, 63, 0]);
    let mut complete = header.clone();
    // DC=0 and AC=EOB consume two zero bits; the remaining six padding bits are ones.
    complete.extend_from_slice(&[0x3f, 0xff, 0xd9]);
    let mut bad_code = header.clone();
    // The only defined DC code is 0. A 16-bit all-ones code is invalid, even
    // when the decoder can read all bits from the actual source instead of padding.
    bad_code.extend_from_slice(&[0xff, 0x00, 0xff, 0x00, 0x7f, 0xff, 0xd9]);
    header.extend_from_slice(&[0xff, 0xd9]);
    println!(
        "minimal JPEG complete/empty sha256={:x}/{:x}",
        Sha256::digest(&complete),
        Sha256::digest(&header)
    );
    let frame = decode(&complete).unwrap();
    assert_eq!((frame.width(), frame.height()), (8, 8));
    assert!(frame.pixels().iter().all(|sample| *sample == 128));
    assert!(matches!(decode(&header), Err(Error::Malformed(_))));
    assert!(matches!(decode(&bad_code), Err(Error::Malformed(_))));
}

#[test]
fn jpeg_icc_after_the_last_scan_is_never_silently_labeled_srgb() {
    for fixture in [BASELINE_JPEG_HEX, PROGRESSIVE_JPEG_HEX] {
        let original = fixture_bytes(fixture);
        assert_eq!(&original[original.len() - 2..], b"\xff\xd9");

        // Reuse the locally generated ICC APP2 from `with_icc_profile`, but move
        // that marker after the final scan and before EOI instead of after SOI.
        let tagged = with_icc_profile(&original);
        assert_eq!(&tagged[2..4], b"\xff\xe2");
        let segment_len = usize::from(u16::from_be_bytes([tagged[4], tagged[5]]));
        let segment = &tagged[2..4 + segment_len];
        let mut late_profile = original[..original.len() - 2].to_vec();
        late_profile.extend_from_slice(segment);
        late_profile.extend_from_slice(b"\xff\xd9");
        println!(
            "late-ICC JPEG fixture sha256={:x}",
            Sha256::digest(&late_profile)
        );

        let decode_limits = limits();
        assert!(matches!(
            decode_with(&late_profile, &decode_limits, &CancellationToken::new()),
            Err(Error::Unsupported(UnsupportedReason::ColorDescription)) | Err(Error::Malformed(_))
        ));
        assert_eq!(decode_limits.resources.cpu_bytes_in_use(), 0);
        assert_eq!(decode_limits.resources.active_decodes(), 0);
        let frame = decode_with(&original, &decode_limits, &CancellationToken::new()).unwrap();
        assert_gray_grid(&frame, (4, 3), SOURCE_SAMPLES);
        drop(frame);
        assert_eq!(decode_limits.resources.cpu_bytes_in_use(), 0);
    }
}

// Change the underlying stream on a rewind after the first scan marker has been read.
struct SwitchAfterScanMarker {
    input: Cursor<Vec<u8>>,
    replacement: Vec<u8>,
    first_sos_end: u64,
    saw_sos_marker: bool,
    switched: bool,
}

impl Read for SwitchAfterScanMarker {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(buffer)?;
        if count != 0 && self.input.position() >= self.first_sos_end {
            self.saw_sos_marker = true;
        }
        Ok(count)
    }
}

impl Seek for SwitchAfterScanMarker {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if matches!(position, SeekFrom::Start(0)) && self.saw_sos_marker && !self.switched {
            self.input.get_mut().copy_from_slice(&self.replacement);
            self.switched = true;
        }
        self.input.seek(position)
    }
}

#[test]
fn jpeg_owned_snapshot_prevents_cross_pass_source_mixing() {
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    let first = with_exif_orientation(&baseline, 1);
    let mut second = with_exif_orientation(&baseline, 2);
    let dqt = second
        .windows(2)
        .position(|marker| marker == b"\xff\xdb")
        .unwrap();
    for value in &mut second[dqt + 5..dqt + 69] {
        *value = value.checked_mul(2).unwrap();
    }
    assert_eq!(first.len(), second.len());

    let expected_first = decode(&first).unwrap();
    let expected_second = decode(&second).unwrap();
    assert_ne!(expected_first.pixels(), expected_second.pixels());
    let first_sos_end = u64::try_from(
        first
            .windows(2)
            .position(|marker| marker == b"\xff\xda")
            .unwrap()
            + 2,
    )
    .unwrap();
    let mut input = SwitchAfterScanMarker {
        input: Cursor::new(first),
        replacement: second,
        first_sos_end,
        saw_sos_marker: false,
        switched: false,
    };
    let frame = decode_image(&mut input, &limits(), &CancellationToken::new()).unwrap();
    assert!(input.saw_sos_marker);
    assert!(!input.switched);
    assert_eq!(frame.pixels(), expected_first.pixels());
}

#[test]
fn documented_jpeg_fixture_sha256_hashes_match() {
    assert_fixture_sha256(
        &fixture_bytes(BASELINE_JPEG_HEX),
        "90f84b4bf806830d877894c41bc6ab5b0688565237094060abeca4b2fdee4982",
    );
    assert_fixture_sha256(
        &fixture_bytes(PROGRESSIVE_JPEG_HEX),
        "ee333a42b8603c25f03293d526ca17352b32b6ad7eaf48e04f77f019f7678581",
    );
    assert_fixture_sha256(
        &fixture_bytes(CANCEL_JPEG_HEX),
        "12870a1e5c0dcfa734a7f00bebb45146696482e158c459afc146d4b0d526166c",
    );

    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    let expected_orientation_hashes = [
        "ea6cc82b7cf4b418300eeb827e9ca22157cdd1134bdb2a2a73ee6d842d3eefec",
        "c776304658b573bfcb2b906be15b02731a0649efb56bdd81274ff62fed42b7b7",
        "a06d5ab8f0c27594a1e7fca500ecb19412256ff19d6d770afcad4de320623258",
        "90fb9b9b9ee9e96ca91b6eed1fae825dfe426a23fcc77ce2dadeace2522451e8",
        "4629a21cd0cc45fe8fc594be0e40fa761c2536f92caac6906b3336785c92bd75",
        "5bc7485e75366ad9e771708914faa9f59a81a27865268d2f1c64fe9d6a997f08",
        "0244a3ecd7fcf76d8627c5522f1f770322fa600276577898733754b7336fd979",
        "73a3b4254d1b5cdb940fba0f8f6b82fea960b29c3114d42c7433adc418a8481f",
    ];
    assert_fixture_sha256(
        &with_icc_profile(&baseline),
        "253e58d9e5253ad0696ab3f4c765e4518e0f82cbfdeb994aafadd42e6b1adbeb",
    );
    for (index, expected_hash) in expected_orientation_hashes.iter().enumerate() {
        let orientation = u16::try_from(index + 1).unwrap();
        let tagged = with_exif_orientation(&baseline, orientation);
        assert_fixture_sha256(&tagged, expected_hash);
    }
}

fn deterministic_progressive_jpeg(width: u32, height: u32) -> Vec<u8> {
    let sample_count = usize::try_from(width.checked_mul(height).expect("fixture dimensions"))
        .expect("fixture fits in memory");
    let mut state = 0x243f_6a88_u32;
    let mut samples = vec![0_u8; sample_count];
    for sample in &mut samples {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *sample = (state >> 24) as u8;
    }

    let mut encoder = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_GRAYSCALE);
    encoder.set_size(
        usize::try_from(width).expect("width fits"),
        usize::try_from(height).expect("height fits"),
    );
    encoder.set_quality(95.0);
    encoder.set_progressive_mode();
    let mut started = encoder.start_compress(Vec::new()).unwrap();
    started.write_scanlines(&samples).unwrap();
    let bytes = started.finish().unwrap();
    assert!(
        bytes
            .windows(2)
            .any(|marker| marker[0] == 0xff && marker[1] == 0xc2)
    );
    bytes
}

fn deterministic_large_jpeg() -> Vec<u8> {
    const WIDTH: u32 = 512;
    const HEIGHT: u32 = 512;
    let mut state = 0x9e37_79b9_u32;
    let sample_count = usize::try_from(WIDTH.checked_mul(HEIGHT).expect("fixture dimensions"))
        .expect("fixture fits in memory");
    let mut samples = vec![0_u8; sample_count];
    for sample in &mut samples {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *sample = (state >> 24) as u8;
    }

    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95)
        .encode(&samples, WIDTH, HEIGHT, image::ExtendedColorType::L8)
        .unwrap();
    bytes
}

fn jpeg_entropy_start(bytes: &[u8]) -> usize {
    let marker = bytes
        .windows(2)
        .position(|pair| pair == b"\xff\xda")
        .expect("JPEG has a Start of Scan marker");
    let segment_length = usize::from(u16::from_be_bytes([bytes[marker + 2], bytes[marker + 3]]));
    marker + 2 + segment_length
}

struct CancelAfterEntropyBytes<'a> {
    inner: Cursor<&'a [u8]>,
    cancellation: &'a CancellationToken,
    cancel_at: u64,
    cancellation_position: Option<u64>,
}

impl Read for CancelAfterEntropyBytes<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(output)?;
        let position = self.inner.position();
        if count != 0 && position >= self.cancel_at && self.cancellation_position.is_none() {
            self.cancellation_position = Some(position);
            self.cancellation.cancel();
        }
        Ok(count)
    }
}

impl Seek for CancelAfterEntropyBytes<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

#[test]
fn cancellation_during_owned_snapshot_read_is_observed() {
    let bytes = fixture_bytes(CANCEL_JPEG_HEX);
    let cancellation = CancellationToken::new();
    let mut input = CancelAfterEntropyBytes {
        inner: Cursor::new(&bytes),
        cancellation: &cancellation,
        cancel_at: 1024,
        cancellation_position: None,
    };
    let decode_limits = limits();
    let result = decode_image(
        &mut input as &mut dyn ReadSeek,
        &decode_limits,
        &cancellation,
    );

    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(cancellation.is_cancelled());
    assert_eq!(decode_limits.resources.cpu_bytes_in_use(), 0);
    assert_eq!(decode_limits.resources.active_decodes(), 0);
    assert!(
        input
            .cancellation_position
            .is_some_and(|position| position >= 1024)
    );
}

#[test]
fn cancellation_during_large_jpeg_snapshot_is_observed() {
    let bytes = deterministic_large_jpeg();
    let entropy_start = jpeg_entropy_start(&bytes);
    let cancel_at = u64::try_from(entropy_start + 16 * 1024).unwrap();
    assert!(u64::try_from(bytes.len()).unwrap() > cancel_at);

    let cancellation = CancellationToken::new();
    let mut input = CancelAfterEntropyBytes {
        inner: Cursor::new(&bytes),
        cancellation: &cancellation,
        cancel_at,
        cancellation_position: None,
    };
    let mut decode_limits = limits();
    decode_limits.budget = Budget::new(4 << 20, 4 << 20, 2 << 20);
    decode_limits.max_decoder_bytes = 4 << 20;
    decode_limits.max_encoded_bytes = 1 << 20;
    let result = decode_image(
        &mut input as &mut dyn ReadSeek,
        &decode_limits,
        &cancellation,
    );

    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(cancellation.is_cancelled());
    assert_eq!(decode_limits.resources.cpu_bytes_in_use(), 0);
    assert_eq!(decode_limits.resources.active_decodes(), 0);
    assert!(
        input
            .cancellation_position
            .is_some_and(|position| position >= cancel_at)
    );
}

#[test]
fn decoder_and_raster_budgets_reject_before_returning_a_raster() {
    let bytes = fixture_bytes(BASELINE_JPEG_HEX);

    let mut decoder_limited = limits();
    decoder_limited.max_decoder_bytes = 11;
    let decoder_result = decode_with(&bytes, &decoder_limited, &CancellationToken::new());
    assert!(matches!(
        decoder_result,
        Err(Error::BudgetExceeded(BudgetKind::RasterBytes))
    ));

    let mut raster_limited = limits();
    raster_limited.budget = Budget::new(1 << 20, 35, 1 << 20);
    let raster_result = decode_with(&bytes, &raster_limited, &CancellationToken::new());
    assert!(matches!(
        raster_result,
        Err(Error::BudgetExceeded(BudgetKind::RasterBytes))
    ));
}

#[test]
fn progressive_jpeg_coefficient_scratch_is_preflighted() {
    const WIDTH: u32 = 512;
    const HEIGHT: u32 = 512;
    let bytes = deterministic_progressive_jpeg(WIDTH, HEIGHT);
    let decode_limits = DecodeLimits::new(
        Budget::new(2 << 20, 2 << 20, 2 << 20),
        DecodeResources::new(4, 16 << 20),
        WIDTH,
        HEIGHT,
        1 << 20,
        512 << 10,
    );

    assert!(bytes.len() < decode_limits.max_encoded_bytes as usize);
    assert!(matches!(
        decode_with(&bytes, &decode_limits, &CancellationToken::new()),
        Err(Error::BudgetExceeded(BudgetKind::RasterBytes))
    ));
    assert_eq!(decode_limits.resources.cpu_bytes_in_use(), 0);
    assert_eq!(decode_limits.resources.active_decodes(), 0);
}

struct CountingInput<'a> {
    inner: Cursor<&'a [u8]>,
    bytes_read: usize,
}

impl Read for CountingInput<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(output)?;
        self.bytes_read += count;
        Ok(count)
    }
}

impl Seek for CountingInput<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

#[test]
fn jpeg_minimum_decoder_buffer_budget_is_checked_before_decoder_construction() {
    let bytes = fixture_bytes(BASELINE_JPEG_HEX);
    let cancellation = CancellationToken::new();

    let mut decoder_limited = limits();
    decoder_limited.max_decoder_bytes = 11;
    let mut decoder_input = CountingInput {
        inner: Cursor::new(&bytes),
        bytes_read: 0,
    };
    let decoder_result = decode_image(
        &mut decoder_input as &mut dyn ReadSeek,
        &decoder_limited,
        &cancellation,
    );
    assert!(matches!(
        decoder_result,
        Err(Error::BudgetExceeded(BudgetKind::RasterBytes))
    ));
    assert_eq!(decoder_input.bytes_read, 12);

    let mut raster_limited = limits();
    raster_limited.budget = Budget::new(1 << 20, 35, 1 << 20);
    let mut raster_input = CountingInput {
        inner: Cursor::new(&bytes),
        bytes_read: 0,
    };
    let raster_result = decode_image(
        &mut raster_input as &mut dyn ReadSeek,
        &raster_limited,
        &CancellationToken::new(),
    );
    assert!(matches!(
        raster_result,
        Err(Error::BudgetExceeded(BudgetKind::RasterBytes))
    ));
    assert_eq!(raster_input.bytes_read, 0);
}

#[test]
fn bounded_jpeg_snapshot_and_codec_scratch_are_distinct_limits() {
    let baseline = fixture_bytes(BASELINE_JPEG_HEX);
    let bytes = with_comment_padding(&baseline, 400 << 10);
    let mut decode_limits = limits();
    decode_limits.budget = Budget::new(1 << 20, 2 << 20, 1 << 20);
    decode_limits.max_decoder_bytes = 256 << 10;
    decode_limits.max_encoded_bytes = 1 << 20;
    assert!(u64::try_from(bytes.len()).unwrap() > decode_limits.max_decoder_bytes);

    let frame = decode_with(&bytes, &decode_limits, &CancellationToken::new()).unwrap();
    assert_gray_grid(&frame, (4, 3), SOURCE_SAMPLES);
    drop(frame);
    assert_eq!(decode_limits.resources.cpu_bytes_in_use(), 0);
    assert_eq!(decode_limits.resources.active_decodes(), 0);
}
