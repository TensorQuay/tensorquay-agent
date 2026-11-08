//! GW-U-041: one closed reason per invalid template field, identical from the file and the write path (ADR-0028,
//! ADR-0032).
use std::error::Error;

use super::{SetRefusal, TemplateInput, validate, validate_set};
use crate::{GatewayConfig, InvalidReason};

const PIN: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn fields(name: &str, extra: &str) -> String {
    format!("name='{name}'\ndisplay_name='T'\nimage='{PIN}'\ncapabilities=['shell']\n{extra}")
}

fn input(toml_fields: &str) -> Result<TemplateInput, Box<dyn Error>> {
    Ok(toml::from_str(toml_fields)?)
}

/// The same templates through the file: the start-up refusal carries the same reason code.
fn file_refusal(templates: &[String]) -> Result<String, Box<dyn Error>> {
    let image = format!("sha256:{}", "a".repeat(64));
    let entries = templates
        .iter()
        .map(|body| format!("[[templates]]\n{body}\n"))
        .collect::<Vec<_>>()
        .concat();
    let text = format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n{entries}"
    );
    match GatewayConfig::from_toml(&text) {
        Ok(_) => Err("the file accepted it".into()),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn gw_u_041_each_invalid_template_field_has_one_reason_from_both_paths()
-> Result<(), Box<dyn Error>> {
    for (body, field, reason) in [
        (fields("Work", ""), "name", InvalidReason::NameInvalid),
        (
            format!("name='t'\ndisplay_name=''\nimage='{PIN}'\ncapabilities=['shell']"),
            "display_name",
            InvalidReason::DisplayNameInvalid,
        ),
        (
            format!(
                "name='t'\ndisplay_name='{}'\nimage='{PIN}'\ncapabilities=['shell']",
                "d".repeat(65)
            ),
            "display_name",
            InvalidReason::DisplayNameInvalid,
        ),
        (
            "name='t'\ndisplay_name='T'\nimage='tqa-runtime:workstation'\ncapabilities=['shell']"
                .to_owned(),
            "image",
            InvalidReason::ImageNotPinned,
        ),
        (
            format!(
                "name='t'\ndisplay_name='T'\nimage='user:secret@registry.example/x@{PIN}'\ncapabilities=['shell']"
            ),
            "image",
            InvalidReason::ImageNotPinned,
        ),
        (
            format!("name='t'\ndisplay_name='T'\nimage='{PIN}'\ncapabilities=['shell','shell']"),
            "capabilities",
            InvalidReason::CapabilitiesInvalid,
        ),
        (
            fields("t", "cpus=0"),
            "cpus",
            InvalidReason::LimitOutOfRange,
        ),
        (
            fields("t", "cpus=65"),
            "cpus",
            InvalidReason::LimitOutOfRange,
        ),
        (
            fields("t", "memory_mib=63"),
            "memory_mib",
            InvalidReason::LimitOutOfRange,
        ),
    ] {
        let refused = validate(input(&body)?).err();
        assert_eq!(refused, Some((field, reason)), "{body}");
        let from_file = file_refusal(&[format!("{body}\ndefault=true")])?;
        assert!(from_file.contains(reason.as_str()), "{from_file}");
    }
    Ok(())
}

#[test]
fn gw_u_041_the_template_set_has_its_own_reasons_from_both_paths() -> Result<(), Box<dyn Error>> {
    let default = |name: &str| fields(name, "default=true");
    let cases: [(Vec<String>, SetRefusal); 4] = [
        (
            vec![fields("a", ""), fields("b", "")],
            SetRefusal::DefaultCount,
        ),
        (vec![default("a"), default("b")], SetRefusal::DefaultCount),
        (vec![default("a"), fields("a", "")], SetRefusal::NameTaken),
        (
            (0..33)
                .map(|index| {
                    if index == 0 {
                        default("t0")
                    } else {
                        fields(&format!("t{index}"), "")
                    }
                })
                .collect(),
            SetRefusal::TemplateCount,
        ),
    ];
    for (bodies, refusal) in cases {
        let templates = bodies
            .iter()
            .map(|body| {
                input(body)
                    .map_err(|error| error.to_string())
                    .and_then(|input| validate(input).map_err(|refused| format!("{refused:?}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(validate_set(&templates), Err(refusal));
        let code = match refusal {
            SetRefusal::NameTaken => "name_taken",
            SetRefusal::DefaultCount => "default_count",
            SetRefusal::TemplateCount => "template_count",
        };
        assert!(file_refusal(&bodies)?.contains(code));
    }
    // A valid set, with 32 templates at most.
    let most: Vec<_> = (0..32)
        .map(|index| {
            if index == 0 {
                default("t0")
            } else {
                fields(&format!("t{index}"), "")
            }
        })
        .map(|body| {
            input(&body)
                .map_err(|error| error.to_string())
                .and_then(|input| validate(input).map_err(|refused| format!("{refused:?}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(validate_set(&most), Ok(()));
    Ok(())
}

// TEST: GW-U-063
#[test]
fn gw_u_063_acknowledged_credential_files_are_absolute_normalised_and_bounded()
-> Result<(), Box<dyn Error>> {
    let template = validate(input(&fields(
        "work",
        "acknowledged_secret_files=['/srv/dev.key', '/etc/ssl/private/test.pem']",
    ))?)
    .map_err(|(field, _)| field)?;
    let acknowledged: Vec<&str> = template
        .acknowledged_secret_files()
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(acknowledged, ["/etc/ssl/private/test.pem", "/srv/dev.key"]);
    let many: Vec<String> = (0..17).map(|index| format!("'/srv/{index}.key'")).collect();
    for extra in [
        "acknowledged_secret_files=['srv/relative.key']".to_owned(),
        "acknowledged_secret_files=['/srv/../etc/key']".to_owned(),
        "acknowledged_secret_files=['/srv//key']".to_owned(),
        "acknowledged_secret_files=['/srv/key/']".to_owned(),
        "acknowledged_secret_files=['/srv/./key']".to_owned(),
        format!("acknowledged_secret_files=[{}]", many.join(",")),
    ] {
        let refused = validate(input(&fields("work", &extra))?).err();
        assert_eq!(
            refused,
            Some(("acknowledged_secret_files", InvalidReason::ParameterInvalid)),
            "{extra}"
        );
    }
    // Through the file the same refusal stops the gateway with its code.
    let message = file_refusal(&[fields(
        "work",
        "default=true\nacknowledged_secret_files=['relative']",
    )])?;
    assert!(message.contains("parameter_invalid"), "{message}");
    Ok(())
}

// TEST: GW-U-080
#[test]
fn gw_u_080_a_template_description_has_one_reason_from_both_paths() -> Result<(), Box<dyn Error>> {
    // Accepted and carried; absent is none.
    let described = validate(input(&fields(
        "t",
        "description='Documents, data and code'",
    ))?)
    .map_err(|refused| format!("{refused:?}"))?;
    assert_eq!(described.description(), Some("Documents, data and code"));
    let plain = validate(input(&fields("t", ""))?).map_err(|refused| format!("{refused:?}"))?;
    assert_eq!(plain.description(), None);
    // Refused the same way from the file and the write path.
    for bad in [
        "description=''",
        "description=' padded'",
        // A TOML basic string, so the escape is a real line break (a literal string would keep the backslash).
        "description=\"two\\nlines\"",
    ] {
        let body = fields("t", bad);
        assert_eq!(
            validate(input(&body)?).err(),
            Some(("description", InvalidReason::DescriptionInvalid)),
            "{body}"
        );
        let from_file = file_refusal(&[format!("{body}\ndefault=true")])?;
        assert!(from_file.contains("description_invalid"), "{from_file}");
    }
    Ok(())
}

/// A whole configuration file with `extra` after `[sandbox]`, then `rest` (templates and other sections).
fn file(extra: &str, rest: &str) -> String {
    let image = format!("sha256:{}", "a".repeat(64));
    format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n{extra}{rest}"
    )
}

// TEST: GW-U-084
#[test]
fn gw_u_084_an_environment_sizes_cpu_in_halves_memory_processes_and_disk()
-> Result<(), Box<dyn Error>> {
    const MIB: u64 = 1024 * 1024;
    for (cpus, millicores) in [("0.5", 500), ("1", 1000), ("1.5", 1500), ("64", 64_000)] {
        let template = validate(input(&fields("t", &format!("cpus={cpus}")))?)
            .map_err(|refusal| format!("{cpus}: {refusal:?}"))?;
        assert_eq!(template.cpu_millicores(), Some(millicores), "{cpus}");
    }
    let sized = validate(input(&fields(
        "t",
        "memory_mib=256\npids=64\nworkspace_mib=16",
    ))?)
    .map_err(|refusal| format!("{refusal:?}"))?;
    assert_eq!(
        (
            sized.memory_bytes(),
            sized.size().pids,
            sized.workspace_bytes()
        ),
        (Some(256 * MIB), Some(64), Some(16 * MIB))
    );
    let unset = validate(input(&fields("t", ""))?).map_err(|refusal| format!("{refusal:?}"))?;
    assert_eq!(
        (
            unset.cpu_millicores(),
            unset.memory_bytes(),
            unset.size().pids,
            unset.workspace_bytes()
        ),
        (None, None, None, None),
        "unset uses the installation's defaults"
    );
    for (extra, field) in [
        ("cpus=0.25", "cpus"),
        ("cpus=0.75", "cpus"),
        ("cpus=0", "cpus"),
        ("cpus=64.5", "cpus"),
        ("cpus=-1", "cpus"),
        ("memory_mib=255", "memory_mib"),
        ("pids=63", "pids"),
        ("pids=65537", "pids"),
        ("workspace_mib=15", "workspace_mib"),
        ("workspace_mib=1048577", "workspace_mib"),
    ] {
        let refused = validate(input(&fields("t", extra))?).err();
        assert_eq!(
            refused,
            Some((field, InvalidReason::LimitOutOfRange)),
            "{extra}"
        );
        let from_file = file_refusal(&[format!("{}\ndefault=true", fields("t", extra))])?;
        assert!(
            from_file.contains("limit_out_of_range"),
            "{extra}: {from_file}"
        );
    }
    Ok(())
}

// TEST: GW-U-084
#[test]
fn gw_u_084_the_installation_ceiling_has_defaults_holds_the_defaults_and_bounds_the_file()
-> Result<(), Box<dyn Error>> {
    const MIB: u64 = 1024 * 1024;
    let plain = GatewayConfig::from_toml(&file("", ""))?;
    let ceiling = &plain.sandbox.ceiling;
    assert_eq!(
        (
            ceiling.cpu_millicores(),
            ceiling.memory_bytes(),
            ceiling.pids(),
            ceiling.workspace_bytes()
        ),
        (8000, 16_384 * MIB, 4096, 102_400 * MIB),
        "the fixed defaults"
    );
    let set = GatewayConfig::from_toml(&file(
        "[sandbox.ceiling]\ncpus=2.5\nmemory_mib=4096\npids=512\nworkspace_mib=20480\n",
        "",
    ))?;
    assert_eq!(set.sandbox.ceiling.cpu_millicores(), 2500);
    // A template above the ceiling stops start-up, naming the ceiling; one at it is fine.
    let template = |extra: &str| {
        format!(
            "[workspaces]\nquota_bytes=1073741824\n\
             [[templates]]\nname='big'\ndisplay_name='Big'\nimage='{PIN}'\ncapabilities=['shell']\ndefault=true\n{extra}"
        )
    };
    let ceiling = "[sandbox.ceiling]\ncpus=2\nmemory_mib=1024\npids=512\nworkspace_mib=2048\n";
    assert!(
        GatewayConfig::from_toml(&file(
            ceiling,
            &template("cpus=2\nmemory_mib=1024\npids=512\nworkspace_mib=1024")
        ))
        .is_ok()
    );
    for above in [
        "cpus=2.5",
        "memory_mib=1025",
        "pids=513",
        "workspace_mib=2049",
    ] {
        let refused = GatewayConfig::from_toml(&file(ceiling, &template(above)))
            .err()
            .ok_or(above)?
            .to_string();
        assert!(
            refused.contains("[sandbox.ceiling]") && refused.contains("limit_out_of_range"),
            "{above}: {refused}"
        );
    }
    // The ceiling holds the defaults, and has bounds of its own.
    for invalid in [
        "cpu_millicores=3000\n[sandbox.ceiling]\ncpus=2\n",
        "pids=600\n[sandbox.ceiling]\npids=512\n",
        "[sandbox.ceiling]\ncpus=0.25\n",
        "[sandbox.ceiling]\ncpus=65\n",
        "[sandbox.ceiling]\nmemory_mib=128\n",
    ] {
        assert!(
            GatewayConfig::from_toml(&file(invalid, "")).is_err(),
            "{invalid}"
        );
    }
    let quota = GatewayConfig::from_toml(&format!(
        "{}[workspaces]\nquota_bytes={}\n",
        file("[sandbox.ceiling]\nworkspace_mib=1024\n", ""),
        2048 * MIB
    ));
    assert!(
        quota.is_err(),
        "the default workspace quota is within the ceiling"
    );
    Ok(())
}

/// GW-U-093: `new` is the Add page's address (`/settings/environments/new`), so no environment may take it as its name:
/// refused from the write path and the file alike, with its own reason; a name that only starts with it is fine.
#[test]
fn gw_u_093_the_name_new_is_reserved_from_both_paths() -> Result<(), Box<dyn Error>> {
    let body = fields("new", "");
    assert_eq!(
        validate(input(&body)?).err(),
        Some(("name", InvalidReason::NameReserved))
    );
    let from_file = file_refusal(&[format!("{body}\ndefault=true")])?;
    assert!(from_file.contains("name_reserved"), "{from_file}");
    for allowed in ["new-tools", "newer", "renew"] {
        assert!(validate(input(&fields(allowed, ""))?).is_ok(), "{allowed}");
    }
    Ok(())
}

// TEST: GW-U-101
/// From a first-hour check: the quick start's Minimal, made from `[runtime] image` when the file has no templates, says
/// what that image (the runtime's base target) has, Shell and Python, as its label and the Environment page do. The
/// wire's legacy default, which older gateways omitted, stays Shell alone.
#[test]
fn gw_u_101_the_quick_start_minimal_is_shell_and_python_like_its_image()
-> Result<(), Box<dyn Error>> {
    let config = GatewayConfig::from_toml(&file("", ""))?;
    let [minimal] = config.templates() else {
        return Err("one template from the runtime image".into());
    };
    assert_eq!(minimal.environment.template, "minimal");
    assert_eq!(
        minimal.environment.capabilities,
        [tqa_types::Capability::Shell, tqa_types::Capability::Python]
    );
    assert_eq!(
        tqa_types::Environment::default().capabilities,
        [tqa_types::Capability::Shell],
        "the legacy default is unchanged"
    );
    Ok(())
}
