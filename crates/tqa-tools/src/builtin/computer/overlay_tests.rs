//! TOOL-U-048: the screenshot's pixels and marks. Xvfb's screen file read as a raster, scaled after drawing, written as
//! a valid PNG by our own encoder, and the overlay: an outline and a label per boxed element, labels inside the screen
//! and apart, at most 300, colours in turn, digits still readable after scaling.
use super::{
    marks::{MAX_MARKS, Mark, Placed, draw},
    png::{crc32, encode_rgb},
    raster::{Raster, Rect},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// An XWD file as Xvfb writes it: a big-endian header, no colour map, 32-bit little-endian pixels.
pub(super) fn xwd(width: u32, height: u32, pixel: impl Fn(u32, u32) -> u32) -> Vec<u8> {
    let header_size = 25 * 4 + 8;
    let line = width * 4 + 4;
    let mut fields = [0_u32; 25];
    fields[0] = header_size;
    fields[1] = 7;
    fields[2] = 2;
    fields[3] = 24;
    fields[4] = width;
    fields[5] = height;
    fields[7] = 0;
    fields[11] = 32;
    fields[12] = line;
    fields[14] = 0x00ff_0000;
    fields[15] = 0x0000_ff00;
    fields[16] = 0x0000_00ff;
    let mut bytes: Vec<u8> = fields
        .iter()
        .flat_map(|field| field.to_be_bytes())
        .collect();
    bytes.extend_from_slice(b"screen0\0");
    for y in 0..height {
        for x in 0..width {
            bytes.extend_from_slice(&pixel(x, y).to_le_bytes());
        }
        bytes.extend_from_slice(&[0; 4]);
    }
    bytes
}

/// The PNG's chunks, each with its CRC checked, and its pixels inflated and unfiltered.
fn decode(png: &[u8]) -> Result<(u32, u32, Vec<u8>), Box<dyn std::error::Error>> {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut at = 8;
    let (mut width, mut height, mut data) = (0, 0, Vec::new());
    let mut kinds = Vec::new();
    while at < png.len() {
        let length = u32::from_be_bytes(png[at..at + 4].try_into()?) as usize;
        let kind = &png[at + 4..at + 8];
        let body = &png[at + 8..at + 8 + length];
        let crc = u32::from_be_bytes(png[at + 8 + length..at + 12 + length].try_into()?);
        assert_eq!(
            crc,
            crc32(&png[at + 4..at + 8 + length]),
            "each chunk's CRC"
        );
        if kind == b"IHDR" {
            width = u32::from_be_bytes(body[..4].try_into()?);
            height = u32::from_be_bytes(body[4..8].try_into()?);
            assert_eq!(&body[8..], &[8, 2, 0, 0, 0], "8-bit RGB, no interlace");
        }
        if kind == b"IDAT" {
            data.extend_from_slice(body);
        }
        kinds.push(String::from_utf8_lossy(kind).into_owned());
        at += 12 + length;
    }
    assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&data)
        .map_err(|error| format!("{error:?}"))?;
    let row = width as usize * 3;
    let mut rgb = Vec::new();
    for line in raw.chunks_exact(row + 1) {
        assert_eq!(line[0], 1, "each row Sub-filtered");
        let start = rgb.len();
        for (index, byte) in line[1..].iter().enumerate() {
            let left = if index >= 3 {
                rgb[start + index - 3]
            } else {
                0
            };
            rgb.push(byte.wrapping_add(left));
        }
    }
    Ok((width, height, rgb))
}

fn rect(x: i32, y: i32, width: u32, height: u32) -> Result<Rect, Box<dyn std::error::Error>> {
    Ok(Rect::new(x, y, width, height).ok_or("an empty rect")?)
}

// TEST: TOOL-U-048
#[test]
fn tool_u_048_the_png_is_valid_and_holds_the_pixels() -> TestResult {
    assert_eq!(
        crc32(b"123456789"),
        0xcbf4_3926,
        "the standard CRC-32 check value"
    );
    let rgb: Vec<u8> = (0..7 * 5 * 3)
        .map(|index| u8::try_from(index * 7 % 256).unwrap_or(0))
        .collect();
    let png = encode_rgb(7, 5, &rgb);
    assert_eq!(decode(&png)?, (7, 5, rgb));
    Ok(())
}

// TEST: TOOL-U-048
#[test]
fn tool_u_048_the_screen_file_reads_as_its_pixels() -> TestResult {
    let file = xwd(3, 2, |x, y| (x * 80) << 16 | (y * 100) << 8 | 7);
    let raster = Raster::from_xwd(&file)?;
    assert_eq!((raster.width, raster.height), (3, 2));
    assert_eq!(raster.pixel(0, 0), [0, 0, 7]);
    assert_eq!(raster.pixel(2, 1), [160, 100, 7]);
    for broken in [
        file[..50].to_vec(),
        file[..file.len() - 10].to_vec(),
        {
            let mut wrong = file.clone();
            wrong[7] = 6;
            wrong
        },
        xwd(0, 2, |_, _| 0),
    ] {
        assert!(Raster::from_xwd(&broken).is_err());
    }
    Ok(())
}

// TEST: TOOL-U-048
#[test]
fn tool_u_048_a_raster_is_scaled_by_averaging_and_only_down() {
    let mut raster = Raster::filled(4, 2, [0, 0, 0]);
    raster.rgb[..6].copy_from_slice(&[100, 100, 100, 200, 200, 200]);
    let half = raster.fitted(2);
    assert_eq!((half.width, half.height), (2, 1));
    assert_eq!(
        half.pixel(0, 0),
        [75, 75, 75],
        "the four pixels it covers, averaged"
    );
    assert_eq!(raster.fitted(10), raster, "never scaled up");
}

fn labels_apart_and_inside(placed: &[Placed], width: u32, height: u32) {
    for (index, mark) in placed.iter().enumerate() {
        assert_eq!(
            mark.label.within(width, height),
            Some(mark.label),
            "label {} inside the screen",
            mark.number
        );
        for other in &placed[index + 1..] {
            assert!(
                !mark.label.overlaps(&other.label),
                "labels {} and {} apart",
                mark.number,
                other.number
            );
        }
    }
}

// TEST: TOOL-U-048
#[test]
fn tool_u_048_each_boxed_element_gets_an_outline_and_a_label() -> TestResult {
    let mut raster = Raster::filled(400, 300, [240, 240, 240]);
    let marks = [
        Mark {
            number: 1,
            rect: rect(0, 0, 80, 30)?,
        },
        Mark {
            number: 2,
            rect: rect(90, 0, 80, 30)?,
        },
        Mark {
            number: 12,
            rect: rect(100, 100, 120, 40)?,
        },
        Mark {
            number: 13,
            rect: rect(-500, -500, 10, 10)?,
        },
        Mark {
            number: 14,
            rect: rect(380, 290, 60, 40)?,
        },
    ];
    let placed = draw(&mut raster, &marks, 0);
    let numbers: Vec<usize> = placed.iter().map(|mark| mark.number).collect();
    assert_eq!(numbers, [1, 2, 12, 14], "a box off the screen gets no mark");
    labels_apart_and_inside(&placed, 400, 300);
    let colours: Vec<[u8; 3]> = placed.iter().map(|mark| mark.colour).collect();
    assert!(
        colours.windows(2).all(|pair| pair[0] != pair[1]),
        "neighbours' colours differ"
    );
    let three = placed[2];
    assert_eq!(
        raster.pixel(219, 120),
        three.colour,
        "the outline's right edge"
    );
    assert_eq!(
        raster.pixel(160, 120),
        [240, 240, 240],
        "the box's inside untouched"
    );
    let label = three.label;
    let inside = (label.x.unsigned_abs()..label.x.unsigned_abs() + label.width).flat_map(|x| {
        (label.y.unsigned_abs()..label.y.unsigned_abs() + label.height).map(move |y| (x, y))
    });
    let white = inside
        .filter(|(x, y)| raster.pixel(*x, *y) == [255, 255, 255])
        .count();
    assert!(white > 0, "the label holds its digits");
    assert_eq!(
        (placed[3].rect.width, placed[3].rect.height),
        (20, 10),
        "clipped to the screen"
    );
    Ok(())
}

// TEST: TOOL-U-048
#[test]
fn tool_u_048_crowded_labels_stay_apart_and_marks_stop_at_300() -> TestResult {
    let mut raster = Raster::filled(1280, 800, [255, 255, 255]);
    let row: Vec<Mark> = (0..12)
        .map(|index| {
            Ok(Mark {
                number: index + 1,
                rect: rect(i32::try_from(index)? * 20, 200, 18, 18)?,
            })
        })
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    labels_apart_and_inside(&draw(&mut raster, &row, 0), 1280, 800);
    let many: Vec<Mark> = (0..400)
        .map(|index| {
            let (column, line) = (index % 20, index / 20);
            Ok(Mark {
                number: index + 1,
                rect: rect(
                    i32::try_from(column)? * 64,
                    i32::try_from(line)? * 40,
                    60,
                    30,
                )?,
            })
        })
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    assert_eq!(draw(&mut raster, &many, 0).len(), MAX_MARKS);
    Ok(())
}

// TEST: TOOL-U-048
#[test]
fn tool_u_048_labels_grow_so_digits_survive_scaling() -> TestResult {
    let marks = [Mark {
        number: 7,
        rect: rect(100, 100, 50, 20)?,
    }];
    let full = draw(&mut Raster::filled(1280, 800, [0, 0, 0]), &marks, 0)[0].label;
    let small = draw(&mut Raster::filled(1280, 800, [0, 0, 0]), &marks, 640)[0].label;
    assert!(
        small.height >= 2 * full.height,
        "{small:?} against {full:?}"
    );
    // A digit is 5 of a label's 7 cells; after halving it is still at least 10 pixels high.
    assert!(small.height * 5 / 7 / 2 >= 10);
    let mut raster = Raster::filled(1280, 800, [0, 0, 0]);
    draw(&mut raster, &marks, 640);
    let shown = raster.fitted(640);
    assert_eq!(
        (shown.width, shown.height),
        (640, 400),
        "scaled after drawing"
    );
    Ok(())
}
