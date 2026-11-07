//! answers-068 Q4: the documented defaults, and bounds that stay ordered.
use super::*;

// TEST: GW-I-013
#[test]
fn gw_i_045_defaults_are_the_documented_limits() -> Result<(), GatewayError> {
    let config = FilesConfig::default().validate()?;
    assert_eq!(config.upload_bytes, 25 * MIB);
    assert_eq!(config.uploads_bytes, GIB);
    assert_eq!(config.preview_bytes, 512 * KIB);
    assert_eq!(config.download_bytes, 100 * MIB);
    assert_eq!(config.upload_seconds, 120);
    // The file previews design: an HTML page, a PDF and a spreadsheet each have their own preview limit.
    assert_eq!(config.preview_html_bytes, 2 * MIB);
    assert_eq!(config.preview_pdf_bytes, 20 * MIB);
    assert_eq!(config.preview_sheet_bytes, 10 * MIB);
    Ok(())
}

// TEST: GW-I-144
// A preview is served no larger than a download: a small download limit caps every preview, with no preview setting.
#[test]
fn gw_i_144_a_preview_limit_is_served_no_larger_than_the_download_limit() -> Result<(), GatewayError>
{
    let config = FilesConfig {
        download_bytes: 4 * KIB,
        preview_bytes: 2 * KIB,
        ..FilesConfig::default()
    }
    .validate()?;
    assert_eq!(config.preview(config.preview_html_bytes), 4 * KIB);
    assert_eq!(config.preview(KIB), KIB);
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_045_bounds_out_of_range_or_order_are_rejected() {
    let base = FilesConfig::default();
    for invalid in [
        FilesConfig {
            upload_bytes: 0,
            ..base
        },
        FilesConfig {
            uploads_bytes: base.upload_bytes - 1,
            ..base
        },
        FilesConfig {
            preview_bytes: 32 * MIB,
            ..base
        },
        FilesConfig {
            download_bytes: base.preview_bytes - 1,
            ..base
        },
        FilesConfig {
            upload_seconds: 5,
            ..base
        },
        // A preview limit below 1 KiB.
        FilesConfig {
            preview_html_bytes: 512,
            ..base
        },
        FilesConfig {
            preview_pdf_bytes: 512,
            ..base
        },
        FilesConfig {
            preview_sheet_bytes: 512,
            ..base
        },
        FilesConfig {
            uploads_bytes: 11 * GIB,
            ..base
        },
    ] {
        assert!(invalid.validate().is_err(), "{invalid:?}");
    }
}

// TEST: GW-I-013
#[test]
fn gw_i_045_toml_is_exact() {
    assert!(toml::from_str::<FilesConfig>("upload_bytes = 2048").is_ok());
    assert!(toml::from_str::<FilesConfig>("unknown = 1").is_err());
}

// The run-changes snapshot bounds.
// TEST: GW-I-013
#[test]
fn gw_i_048_snapshot_defaults_are_the_documented_bounds() -> Result<(), GatewayError> {
    let config = FilesConfig::default().validate()?;
    assert_eq!(config.snapshot_files, 5000);
    assert_eq!(config.snapshot_entries, 20_000);
    assert_eq!(config.snapshot_text_bytes, 256 * KIB);
    assert_eq!(config.snapshot_copy_bytes, 32 * MIB);
    assert_eq!(config.snapshots_kept, 20);
    assert_eq!(config.snapshot_seconds, 30);
    assert_eq!(config.diff_bytes, 512 * KIB);
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_snapshot_bounds_out_of_range_or_order_are_rejected() {
    let base = FilesConfig::default();
    for invalid in [
        FilesConfig {
            snapshot_files: 0,
            ..base
        },
        FilesConfig {
            snapshot_files: 100_001,
            ..base
        },
        FilesConfig {
            snapshot_entries: 99,
            ..base
        },
        FilesConfig {
            snapshot_entries: 1_000_001,
            ..base
        },
        FilesConfig {
            snapshot_text_bytes: 17 * MIB,
            ..base
        },
        FilesConfig {
            snapshot_copy_bytes: base.snapshot_text_bytes - 1,
            ..base
        },
        FilesConfig {
            snapshots_kept: 1,
            ..base
        },
        FilesConfig {
            snapshot_seconds: 0,
            ..base
        },
        FilesConfig {
            diff_bytes: 17 * MIB,
            ..base
        },
    ] {
        assert!(invalid.validate().is_err(), "{invalid:?}");
    }
}

#[test]
fn gw_i_081_link_requests_default_to_eight_within_one_to_sixty_four() -> Result<(), GatewayError> {
    let base = FilesConfig::default().validate()?;
    assert_eq!(base.link_requests, 8);
    for (link_requests, valid) in [(0, false), (1, true), (64, true), (65, false)] {
        let config = FilesConfig {
            link_requests,
            ..base
        };
        assert_eq!(config.validate().is_ok(), valid, "{link_requests}");
    }
    Ok(())
}
