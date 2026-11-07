// TEST: SBX-U-011
use std::{error::Error, fmt::Write as _, io::Write};

use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};

use super::{Bound, Bounds, ExportError, ScanError, scan_export};

type TestResult = Result<(), Box<dyn Error>>;

/// A layer tar with one file per (path, content).
fn layer(files: &[(&str, &[u8])]) -> std::io::Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, data) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, *data)?;
    }
    builder.into_inner()
}

fn hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

fn diff_id(layer: &[u8]) -> String {
    format!("sha256:{}", hex(layer))
}

fn gzip(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(data)?;
    encoder.finish()
}

/// An engine export: each (name, bytes) as one entry, in the order given.
fn export(entries: &[(String, Vec<u8>)]) -> std::io::Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, data.as_slice())?;
    }
    builder.into_inner()
}

/// A private key's shape, assembled so the repository's own scanners never see a key-like literal.
fn key() -> Vec<u8> {
    format!(
        "-----BEGIN {}-----\nMIIB\n-----END {}-----\n",
        "PRIVATE KEY", "PRIVATE KEY"
    )
    .into_bytes()
}

struct Image {
    base: Vec<u8>,
    own: Vec<u8>,
}

fn image() -> Result<Image, Box<dyn Error>> {
    Ok(Image {
        base: layer(&[
            ("etc/ssl/private/base.key", &key()),
            ("etc/os-release", b"ID=debian\n"),
        ])?,
        own: layer(&[
            ("srv/app/server.key", &key()),
            ("srv/app/main.py", b"print(1)\n"),
        ])?,
    })
}

#[test]
fn only_the_templates_own_layer_is_a_hit_in_the_oci_layout() -> TestResult {
    let image = image()?;
    // Blobs are named by their compressed digest and listed before the index, as the engine writes them.
    let blobs = [gzip(&image.own)?, gzip(&image.base)?];
    let mut entries: Vec<(String, Vec<u8>)> = blobs
        .iter()
        .map(|blob| (format!("blobs/sha256/{}", hex(blob)), blob.clone()))
        .collect();
    entries.push((
        "blobs/sha256/0f".into(),
        br#"{"config":"not a layer"}"#.to_vec(),
    ));
    entries.push(("index.json".into(), b"{}".to_vec()));
    entries.push(("manifest.json".into(), b"[]".to_vec()));
    let found = scan_export(
        export(&entries)?.as_slice(),
        &[diff_id(&image.own)],
        Bounds::default(),
    )?;
    assert_eq!(found.paths(), ["/srv/app/server.key"]);
    Ok(())
}

#[test]
fn the_classic_layout_with_uncompressed_layers_reads_the_same() -> TestResult {
    let image = image()?;
    let entries = vec![
        ("aaa/layer.tar".to_owned(), image.base.clone()),
        ("bbb/layer.tar".to_owned(), image.own.clone()),
        ("manifest.json".to_owned(), b"[]".to_vec()),
    ];
    let found = scan_export(
        export(&entries)?.as_slice(),
        &[diff_id(&image.own)],
        Bounds::default(),
    )?;
    assert_eq!(found.paths(), ["/srv/app/server.key"]);
    Ok(())
}

#[test]
fn own_layers_apply_in_the_images_order_whatever_the_exports_order() -> TestResult {
    let first = layer(&[("srv/app/server.key", &key())])?;
    let second = layer(&[("srv/app/.wh.server.key", b"")])?;
    // The export lists the whiteout layer first; the image's order puts it last, so the key is gone.
    let entries = vec![
        ("blobs/sha256/1".to_owned(), gzip(&second)?),
        ("blobs/sha256/2".to_owned(), gzip(&first)?),
    ];
    let own = [diff_id(&first), diff_id(&second)];
    let found = scan_export(export(&entries)?.as_slice(), &own, Bounds::default())?;
    assert_eq!(found.total, 0);
    Ok(())
}

#[test]
fn a_template_that_is_its_own_base_scans_nothing() -> TestResult {
    let image = image()?;
    let entries = vec![("blobs/sha256/1".to_owned(), gzip(&image.base)?)];
    let found = scan_export(export(&entries)?.as_slice(), &[], Bounds::default())?;
    assert_eq!(found.total, 0);
    Ok(())
}

#[test]
fn a_missing_own_layer_or_a_zstd_blob_is_unavailable_never_clean() -> TestResult {
    let image = image()?;
    let entries = vec![("blobs/sha256/1".to_owned(), gzip(&image.base)?)];
    let missing = scan_export(
        export(&entries)?.as_slice(),
        &[diff_id(&image.own)],
        Bounds::default(),
    );
    assert_eq!(missing, Err(ExportError::Unavailable));
    let mut zstd = vec![0x28, 0xb5, 0x2f, 0xfd];
    zstd.extend_from_slice(&[0; 64]);
    let entries = vec![
        ("blobs/sha256/1".to_owned(), zstd),
        ("blobs/sha256/2".to_owned(), gzip(&image.base)?),
    ];
    let unknown = scan_export(
        export(&entries)?.as_slice(),
        &[diff_id(&image.own)],
        Bounds::default(),
    );
    assert_eq!(unknown, Err(ExportError::Unavailable));
    Ok(())
}

#[test]
fn a_base_layer_past_the_bound_is_not_the_templates_problem() -> TestResult {
    let big: Vec<(String, Vec<u8>)> = (0..20)
        .map(|index| (format!("usr/share/{index}"), vec![0; 10]))
        .collect();
    let files: Vec<(&str, &[u8])> = big
        .iter()
        .map(|(path, data)| (path.as_str(), data.as_slice()))
        .collect();
    let base = layer(&files)?;
    let own = layer(&[("srv/app/server.key", &key())])?;
    let entries = vec![
        ("blobs/sha256/1".to_owned(), gzip(&base)?),
        ("blobs/sha256/2".to_owned(), gzip(&own)?),
    ];
    let bounds = Bounds {
        max_entries: 5,
        max_bytes: 1 << 20,
    };
    let found = scan_export(export(&entries)?.as_slice(), &[diff_id(&own)], bounds)?;
    assert_eq!(found.paths(), ["/srv/app/server.key"]);
    // The same layer as the template's own is past the bound: a limit, never clean.
    let over = scan_export(export(&entries)?.as_slice(), &[diff_id(&base)], bounds);
    assert_eq!(
        over,
        Err(ExportError::Scan(ScanError::Limit(Bound::Entries)))
    );
    Ok(())
}

#[test]
fn a_corrupt_gzip_own_layer_is_unavailable() -> TestResult {
    let image = image()?;
    let mut blob = gzip(&image.own)?;
    let middle = blob.len() / 2;
    blob[middle] ^= 0xff;
    let entries = vec![("blobs/sha256/1".to_owned(), blob)];
    let result = scan_export(
        export(&entries)?.as_slice(),
        &[diff_id(&image.own)],
        Bounds::default(),
    );
    assert!(
        matches!(
            result,
            Err(ExportError::Unavailable | ExportError::Scan(ScanError::Malformed))
        ),
        "{result:?}"
    );
    Ok(())
}

/// The scan's wall time on a real engine export read from standard input, for the bound's design note:
/// `docker save <image> | TQA_SCAN_OWN_LAYERS=<id,id,...> cargo test --release -p tqa-sandbox --lib measure -- --ignored`
#[test]
#[ignore = "measures a real engine export read from standard input"]
fn measure_a_real_export() -> TestResult {
    // A measurement, not a check: without its input (as under `--run-ignored all`) it has nothing to measure.
    let Ok(layers) = std::env::var("TQA_SCAN_OWN_LAYERS") else {
        writeln!(
            std::io::stdout(),
            "skipped: set TQA_SCAN_OWN_LAYERS and pipe an export to measure"
        )?;
        return Ok(());
    };
    let own: Vec<String> = layers.split(',').map(str::to_owned).collect();
    let started = std::time::Instant::now();
    let found = scan_export(std::io::stdin().lock(), &own, Bounds::default())?;
    writeln!(
        std::io::stdout(),
        "scanned an export with {} own layers in {:.1} s: {} hits",
        own.len(),
        started.elapsed().as_secs_f64(),
        found.total
    )?;
    Ok(())
}
