//! The guest deny-list: the system calls the Docker default profile refuses to an unprivileged container, refused
//! here with `EPERM` so both backends present the same posture (SBX-C-007). Everything else is allowed, including
//! same-uid `ptrace` (ARCHITECTURE §12). User namespaces are disabled by sysctl in stage one, so `clone` needs no
//! flag rules and `clone3` stays usable.

use std::collections::BTreeMap;

use nix::libc;
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch,
};

use crate::error::InitError;

/// Refused outright, whatever the arguments.
#[cfg(target_arch = "x86_64")]
pub const DENIED: &[i64] = &[
    libc::SYS_acct,
    libc::SYS_add_key,
    libc::SYS_bpf,
    libc::SYS_clock_adjtime,
    libc::SYS_clock_settime,
    libc::SYS_delete_module,
    libc::SYS_finit_module,
    libc::SYS_fsconfig,
    libc::SYS_fsmount,
    libc::SYS_fsopen,
    libc::SYS_fspick,
    libc::SYS_get_mempolicy,
    libc::SYS_init_module,
    libc::SYS_io_uring_enter,
    libc::SYS_io_uring_register,
    libc::SYS_io_uring_setup,
    libc::SYS_ioperm,
    libc::SYS_iopl,
    libc::SYS_kcmp,
    libc::SYS_kexec_file_load,
    libc::SYS_kexec_load,
    libc::SYS_keyctl,
    libc::SYS_lookup_dcookie,
    libc::SYS_mbind,
    libc::SYS_mount,
    libc::SYS_mount_setattr,
    libc::SYS_move_mount,
    libc::SYS_move_pages,
    libc::SYS_name_to_handle_at,
    libc::SYS_nfsservctl,
    libc::SYS_open_by_handle_at,
    libc::SYS_open_tree,
    libc::SYS_perf_event_open,
    libc::SYS_pivot_root,
    libc::SYS_quotactl,
    libc::SYS_quotactl_fd,
    libc::SYS_reboot,
    libc::SYS_request_key,
    libc::SYS_set_mempolicy,
    libc::SYS_setns,
    libc::SYS_settimeofday,
    libc::SYS_swapoff,
    libc::SYS_swapon,
    libc::SYS_sysfs,
    libc::SYS__sysctl,
    libc::SYS_umount2,
    libc::SYS_unshare,
    libc::SYS_uselib,
    libc::SYS_userfaultfd,
    libc::SYS_ustat,
    libc::SYS_vhangup,
];

/// `personality` values the Docker default profile permits; any other value is refused.
const PERSONALITIES: [u64; 5] = [0x0, 0x8, 0x2_0000, 0x2_0008, 0xffff_ffff];

/// Compiles the deny-list for this architecture.
///
/// # Errors
/// Returns `Step` if the filter cannot be built or compiled.
pub fn program() -> Result<BpfProgram, InitError> {
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> =
        DENIED.iter().map(|&call| (call, Vec::new())).collect();
    let conditions = PERSONALITIES
        .iter()
        .map(|&value| SeccompCondition::new(0, SeccompCmpArgLen::Dword, SeccompCmpOp::Ne, value))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| InitError::Step("seccomp personality rule"))?;
    let personality =
        SeccompRule::new(conditions).map_err(|_| InitError::Step("seccomp personality rule"))?;
    rules.insert(libc::SYS_personality, vec![personality]);
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM.cast_unsigned()),
        TargetArch::x86_64,
    )
    .map_err(|_| InitError::Step("seccomp filter"))?;
    filter
        .try_into()
        .map_err(|_| InitError::Step("seccomp compile"))
}

/// Installs the filter on the calling thread; it is inherited across `exec` and by every child.
///
/// # Errors
/// Returns `Step` if compilation or installation fails; the caller must not continue unfiltered.
pub fn install() -> Result<(), InitError> {
    seccompiler::apply_filter(&program()?).map_err(|_| InitError::Step("seccomp install"))
}

#[cfg(test)]
mod tests {
    use std::{error::Error, thread};

    use nix::{
        errno::Errno,
        libc,
        sys::{personality, prctl},
    };

    use super::{DENIED, install, program};

    #[test]
    fn sbx_f_016_deny_list_covers_the_boundary_and_spares_debugging() -> Result<(), Box<dyn Error>>
    {
        assert!(!program()?.is_empty());
        for required in [
            libc::SYS_mount,
            libc::SYS_unshare,
            libc::SYS_setns,
            libc::SYS_bpf,
            libc::SYS_keyctl,
            libc::SYS_kexec_load,
            libc::SYS_init_module,
            libc::SYS_perf_event_open,
            libc::SYS_io_uring_setup,
        ] {
            assert!(DENIED.contains(&required), "{required} must be refused");
        }
        for allowed in [
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_clone,
            libc::SYS_clone3,
            libc::SYS_execve,
            libc::SYS_setsid,
        ] {
            assert!(!DENIED.contains(&allowed), "{allowed} must stay available");
        }
        Ok(())
    }

    #[test]
    fn sbx_f_016_installed_filter_refuses_a_call_that_is_otherwise_permitted()
    -> Result<(), Box<dyn Error>> {
        // The filter binds only the thread that installs it, so the test process stays unfiltered.
        let outcome = thread::spawn(|| -> Result<_, String> {
            let base = personality::get().map_err(|error| error.to_string())?;
            let unfiltered = personality::set(base | personality::Persona::ADDR_NO_RANDOMIZE);
            personality::set(base).map_err(|error| error.to_string())?;
            prctl::set_no_new_privs().map_err(|error| error.to_string())?;
            install().map_err(|error| error.to_string())?;
            let filtered = personality::set(base | personality::Persona::ADDR_NO_RANDOMIZE);
            Ok((unfiltered.is_ok(), filtered, personality::get().is_ok()))
        })
        .join()
        .map_err(|_| "filter thread panicked")??;
        assert!(outcome.0, "control: the call succeeds without the filter");
        assert_eq!(outcome.1.err(), Some(Errno::EPERM));
        assert!(outcome.2, "querying the persona is still allowed");
        Ok(())
    }
}
