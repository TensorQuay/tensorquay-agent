// TEST: GW-U-063
use super::{TemplateChecksInput, template_checks};

fn parsed(toml: &str) -> Result<TemplateChecksInput, toml::de::Error> {
    toml::from_str(toml)
}

#[test]
fn the_defaults_are_the_measured_bounds() -> Result<(), Box<dyn std::error::Error>> {
    let bounds = template_checks(parsed("")?)?;
    assert_eq!((bounds.max_entries, bounds.max_bytes), (160_000, 6 << 30));
    let raised = template_checks(parsed(
        "max_scan_entries = 400000\nmax_scan_bytes = 17179869184",
    )?)?;
    assert_eq!((raised.max_entries, raised.max_bytes), (400_000, 16 << 30));
    Ok(())
}

#[test]
fn bounds_outside_their_ranges_stop_the_gateway() -> Result<(), Box<dyn std::error::Error>> {
    for toml in [
        "max_scan_entries = 999",
        "max_scan_entries = 100000001",
        "max_scan_bytes = 1048575",
        "max_scan_bytes = 1099511627777",
    ] {
        assert!(template_checks(parsed(toml)?).is_err(), "{toml}");
    }
    assert!(parsed("unknown = 1").is_err());
    Ok(())
}
