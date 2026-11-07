//! Guest init for microVM sandboxes (ADR-0006). As PID 1 it mounts the guest's storage, delivers the per-run token
//! as a file, applies limits, and starts stage two in the sandbox's PID namespace. Stage two drops every privilege,
//! installs the seccomp filter and executes the image's entrypoint. The VM powers off when the sandbox ends.
#![forbid(unsafe_code)]

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!("tqa-vm-init supports x86_64 Linux guests only");

mod config;
mod error;
mod exit_record;
mod plan;
mod sandbox_dev;
mod seccomp;
mod stage1;
mod stage2;

use std::{io::Write, process::ExitCode};

/// Writes one bounded line to the console; error texts are static and never carry configuration values.
fn console(line: std::fmt::Arguments<'_>) {
    let _ = writeln!(std::io::stderr(), "tqa-vm-init: {line}");
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((mode, rest)) if mode == "--stage2" => {
            let error = match stage2::Stage2::parse(rest).and_then(|stage| stage.run()) {
                Err(error) => error,
                Ok(never) => match never {},
            };
            console(format_args!("{error}"));
            ExitCode::from(125)
        }
        None if std::process::id() == 1 => {
            let code = match stage1::run() {
                Ok(status) => {
                    console(format_args!("sandbox exited: {status}"));
                    exit_record::code(status)
                }
                Err(error) => {
                    console(format_args!("{error}"));
                    exit_record::INIT_FAILED
                }
            };
            if let Err(error) = exit_record::write(code) {
                console(format_args!("{error}"));
            }
            stage1::power_off()
        }
        _ => {
            console(format_args!("runs only as the guest's PID 1"));
            ExitCode::from(2)
        }
    }
}
