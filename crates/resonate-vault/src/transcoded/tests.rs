use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

use image::{ImageEncoder as _, codecs::jpeg::JpegEncoder};

use super::*;
use crate::cover;

fn photographic(width: u32, height: u32) -> Vec<u8> {
    let mut state = 0x9e37_79b9_u32;
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let grain = (state >> 24) as f32 / 255.0 - 0.5;
            let (fx, fy) = (x as f32 / width as f32, y as f32 / height as f32);
            let sky = 0.55 + 0.35 * (fy * 3.1).cos();
            let hill = 0.5 + 0.4 * ((fx * 5.0).sin() * (fy * 4.0).cos());
            let edge = if (fx - 0.5).powi(2) + (fy - 0.45).powi(2) < 0.06 {
                0.3
            } else {
                0.0
            };
            let red = sky * 0.9 + edge + grain * 0.12;
            let green = hill * 0.8 + grain * 0.12;
            let blue = sky + hill * 0.2 - edge + grain * 0.12;
            for sample in [red, green, blue] {
                pixels.push((sample.clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
    }
    pixels
}

fn baseline(width: u32, height: u32, quality: u8) -> Vec<u8> {
    let mut written = Vec::new();
    JpegEncoder::new_with_quality(&mut written, quality)
        .write_image(
            &photographic(width, height),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        )
        .expect("a written JPEG");
    written
}

fn scratch(name: &str) -> PathBuf {
    static MADE: AtomicUsize = AtomicUsize::new(0);
    let folder = std::env::temp_dir().join(format!(
        "resonate-transcoded-{name}-{}-{}",
        std::process::id(),
        MADE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&folder).expect("a folder");
    folder
}

fn progressive(baseline: &[u8]) -> Option<Vec<u8>> {
    let folder = scratch("progressive");
    let input = folder.join("in.jpg");
    let output = folder.join("out.jpg");
    fs::write(&input, baseline).expect("a written JPEG");
    let ran = Command::new("jpegtran")
        .args(["-progressive", "-outfile"])
        .arg(&output)
        .arg(&input)
        .stderr(Stdio::null())
        .status()
        .ok()?;
    let read = ran.success().then(|| fs::read(&output).ok()).flatten();
    fs::remove_dir_all(&folder).expect("a removed folder");
    read
}

fn subsampled(width: u32, height: u32, quality: u8) -> Option<Vec<u8>> {
    let folder = scratch("subsampled");
    let input = folder.join("in.png");
    let output = folder.join("out.jpg");
    image::RgbImage::from_raw(width, height, photographic(width, height))
        .expect("a picture")
        .save(&input)
        .expect("a written PNG");
    let ran = Command::new("convert")
        .arg(&input)
        .args([
            "-sampling-factor",
            "4:2:0",
            "-quality",
            &quality.to_string(),
        ])
        .arg(&output)
        .stderr(Stdio::null())
        .status()
        .ok()?;
    let read = ran.success().then(|| fs::read(&output).ok()).flatten();
    fs::remove_dir_all(&folder).expect("a removed folder");
    read
}

fn grey(width: u32, height: u32, quality: u8) -> Vec<u8> {
    let colour = photographic(width, height);
    let pixels = colour
        .as_chunks::<3>()
        .0
        .iter()
        .map(|[_, green, _]| *green)
        .collect::<Vec<_>>();
    let mut written = Vec::new();
    JpegEncoder::new_with_quality(&mut written, quality)
        .write_image(&pixels, width, height, image::ExtendedColorType::L8)
        .expect("a written JPEG");
    written
}

pub(crate) fn with_exif(jpeg: &[u8]) -> Vec<u8> {
    let mut exif = b"Exif\0\0II*\0\x08\0\0\0\0\0".to_vec();
    exif.extend_from_slice(&[7_u8; 40]);
    spliced(jpeg, 0xe1, &exif)
}

pub(crate) fn with_comment(jpeg: &[u8]) -> Vec<u8> {
    spliced(jpeg, 0xfe, b"a cover comment")
}

fn spliced(jpeg: &[u8], marker: u8, payload: &[u8]) -> Vec<u8> {
    const START_OF_IMAGE: usize = 2;
    let behind_the_first_segment =
        START_OF_IMAGE + 2 + usize::from(u16::from_be_bytes([jpeg[4], jpeg[5]]));

    let mut spliced = jpeg[..behind_the_first_segment].to_vec();
    spliced.extend_from_slice(&[0xff, marker]);
    spliced.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    spliced.extend_from_slice(payload);
    spliced.extend_from_slice(&jpeg[behind_the_first_segment..]);
    spliced
}

fn trailing(jpeg: &[u8]) -> Vec<u8> {
    let mut longer = jpeg.to_vec();
    longer.extend_from_slice(b"bytes after the end of image");
    longer
}

fn percent(kept: usize, was: usize) -> f64 {
    (kept as f64 / was as f64 - 1.0) * 100.0
}

#[test]
fn a_transcode_reconstructs_the_jpeg_it_came_from_byte_for_byte_and_is_smaller_from_300_pixels() {
    let mut rows = Vec::new();
    for (side, quality) in [(128, 60), (300, 75), (300, 92), (600, 85), (1000, 90)] {
        let plain = baseline(side, side, quality);
        let mut kinds = vec![("baseline 4:4:4", plain.clone())];
        kinds.extend(subsampled(side, side, quality).map(|held| ("baseline 4:2:0", held)));
        kinds.push(("grey", grey(side, side, quality)));
        for (kind, jpeg) in kinds {
            let began = std::time::Instant::now();
            let transcoded = of(&jpeg).expect("a transcoded JPEG");
            let transcoding = began.elapsed();
            let began = std::time::Instant::now();
            let exact = reconstructs(&transcoded.bytes, &jpeg);
            let reconstructing = began.elapsed();
            assert!(exact, "{side}px q{quality} {kind} did not reconstruct");
            assert_eq!((transcoded.width, transcoded.height), (side, side));
            if side >= 300 {
                assert!(
                    transcoded.bytes.len() < jpeg.len(),
                    "{side}px q{quality} {kind} grew"
                );
            }
            rows.push(format!(
                "{side}px q{quality} {kind}: jpeg {} jxl {} ({:+.1}%), transcoded in {transcoding:?}, reconstructed in {reconstructing:?}",
                jpeg.len(),
                transcoded.bytes.len(),
                percent(transcoded.bytes.len(), jpeg.len()),
            ));
        }
    }
    println!("{}", rows.join("\n"));
}

#[test]
fn an_exif_segment_and_bytes_after_the_image_survive_the_round_trip() {
    let plain = baseline(200, 150, 80);

    for (name, jpeg) in [
        ("exif", with_exif(&plain)),
        ("trailing", trailing(&plain)),
        ("both", trailing(&with_exif(&plain))),
    ] {
        let transcoded = of(&jpeg).expect("a transcoded JPEG");

        assert!(reconstructs(&transcoded.bytes, &jpeg), "{name}");
    }
}

#[test]
fn a_comment_segment_is_written_back_one_byte_long_so_the_jpeg_is_not_reconstructed() {
    let jpeg = with_comment(&baseline(200, 150, 80));

    let transcoded = of(&jpeg).expect("a transcoded JPEG");

    assert!(!reconstructs(&transcoded.bytes, &jpeg));
}

#[test]
fn a_transcode_without_its_seal_or_with_a_flipped_bit_is_refused_before_it_is_reconstructed() {
    let jpeg = baseline(200, 150, 80);
    let transcoded = of(&jpeg).expect("a transcoded JPEG");
    let unsealed = &transcoded.bytes[..transcoded.bytes.len() - SEAL_BYTES];

    assert!(reconstructs(&transcoded.bytes, &jpeg));
    assert!(!reconstructs(unsealed, &jpeg));
    assert!(
        cover::opened_jxl(unsealed)
            .and_then(|picture| reconstructed(unsealed, &picture))
            .is_err()
    );
}

#[test]
fn a_progressive_jpeg_is_not_transcoded() {
    let Some(progressive) = progressive(&baseline(300, 300, 85)) else {
        return;
    };

    assert!(of(&progressive).is_none());
}

#[test]
fn a_jxl_holding_pixels_alone_reconstructs_no_jpeg() {
    let png = {
        let mut written = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_raw(32, 32, photographic(32, 32))
            .expect("a picture")
            .write_to(&mut written, image::ImageFormat::Png)
            .expect("a written PNG");
        resonate_codec::CoverArt {
            format: resonate_codec::ImageFormat::Png,
            bytes: written.into_inner(),
        }
    };
    let cover::Drawing::Lossless(drawn) = cover::as_jxl(&png).expect("a drawing") else {
        panic!("a plain picture was kept as it came");
    };
    let opened = cover::opened_jxl(&drawn.bytes).expect("an opened JXL");

    assert!(
        reconstructed(&drawn.bytes, &opened)
            .expect("a verdict")
            .is_none()
    );
}

#[test]
fn a_damaged_jpeg_is_never_a_panic_and_a_damaged_transcode_never_reconstructs_the_original() {
    let seeds = [
        baseline(96, 80, 80),
        grey(64, 48, 70),
        subsampled(96, 80, 75).unwrap_or_else(|| baseline(80, 96, 75)),
    ];
    let mut state = 0x2545_f491_u32;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as usize
    };

    for round in 0..300 {
        let jpeg = &seeds[round % seeds.len()];
        let mut damaged = jpeg.clone();
        for _ in 0..1 + next() % 3 {
            let reach = if next() % 2 == 0 { 700 } else { damaged.len() };
            let at = next() % damaged.len().min(reach);
            damaged[at] = next() as u8;
        }
        if let Some(again) = of(&damaged) {
            assert!(!reconstructs(&again.bytes, jpeg) || damaged == *jpeg);
        }
    }

    let jpeg = &seeds[0];
    let transcoded = of(jpeg).expect("a transcoded JPEG");
    for _ in 0..300 {
        let mut damaged = transcoded.bytes.clone();
        let at = next() % damaged.len();
        damaged[at] ^= 1 << (next() % 8);
        assert!(!reconstructs(&damaged, jpeg));
    }
}
