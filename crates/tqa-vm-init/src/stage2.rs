//! Stage two is PID 1 of the sandbox's PID namespace. It gives the sandbox a private `/proc`, joins the sandbox
//! cgroup, drops every privilege, installs the filter and becomes the image's entrypoint.

use std::{convert::Infallible, fs, os::unix::process::CommandExt, process::Command};

use caps::CapSet;
use nix::{
    mount::{MsFlags, mount},
    sched::{CloneFlags, unshare},
    sys::prctl,
    unistd::{Gid, Uid, getpid, setgroups, setresgid, setresuid},
};

use crate::{
    error::{InitError, io, sys},
    plan, seccomp,
};

/// Parsed stage-two arguments: `uid gid -- program args...`.
#[derive(Debug, Eq, PartialEq)]
pub struct Stage2 {
    uid: u32,
    gid: u32,
    argv: Vec<String>,
}

impl Stage2 {
    /// Parses the arguments stage one passed.
    ///
    /// # Errors
    /// Returns `Image` for a malformed list, a root identity or an empty command.
    pub fn parse(args: &[String]) -> Result<Self, InitError> {
        let bad = InitError::Image("stage two arguments");
        let [user, group, separator, command @ ..] = args else {
            return Err(bad);
        };
        let (uid, gid) = (
            user.parse().map_err(|_| InitError::Image("uid"))?,
            group.parse().map_err(|_| InitError::Image("gid"))?,
        );
        if separator != "--" || command.is_empty() || uid == 0 || gid == 0 {
            return Err(bad);
        }
        Ok(Self {
            uid,
            gid,
            argv: command.to_vec(),
        })
    }

    /// Enters the sandbox and executes the entrypoint; returns only on failure.
    ///
    /// # Errors
    /// Returns the failing step; nothing is executed unless every step succeeded.
    pub fn run(&self) -> Result<Infallible, InitError> {
        if getpid().as_raw() != 1 {
            return Err(InitError::Step("stage two outside its PID namespace"));
        }
        unshare(CloneFlags::CLONE_NEWNS).map_err(sys("mount namespace"))?;
        mount(
            None::<&str>,
            "/",
            None::<&str>,
            MsFlags::MS_REC | MsFlags::MS_PRIVATE,
            None::<&str>,
        )
        .map_err(sys("private mounts"))?;
        let flags = MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC;
        mount(Some("proc"), "/proc", Some("proc"), flags, None::<&str>)
            .map_err(sys("sandbox proc"))?;
        crate::sandbox_dev::build()?;
        fs::write(format!("{}/cgroup.procs", plan::SANDBOX_CGROUP), "0")
            .map_err(io("join sandbox cgroup"))?;
        self.drop_privileges()?;
        prctl::set_no_new_privs().map_err(sys("no_new_privs"))?;
        seccomp::install()?;
        let error = Command::new(&self.argv[0]).args(&self.argv[1..]).exec();
        Err(io("exec entrypoint")(error))
    }

    fn drop_privileges(&self) -> Result<(), InitError> {
        for set in [CapSet::Ambient, CapSet::Inheritable, CapSet::Bounding] {
            caps::clear(None, set).map_err(|_| InitError::Step("clear capabilities"))?;
        }
        let (uid, gid) = (Uid::from_raw(self.uid), Gid::from_raw(self.gid));
        setgroups(&[]).map_err(sys("clear groups"))?;
        setresgid(gid, gid, gid).map_err(sys("set group"))?;
        setresuid(uid, uid, uid).map_err(sys("set user"))?;
        for set in [CapSet::Effective, CapSet::Permitted] {
            if !caps::read(None, set)
                .map_err(|_| InitError::Step("read capabilities"))?
                .is_empty()
            {
                return Err(InitError::Step("capabilities survived the user change"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Stage2;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|&arg| arg.to_owned()).collect()
    }

    #[test]
    fn sbx_f_016_stage_two_arguments() {
        assert_eq!(
            Stage2::parse(&args(&["1000", "1000", "--", "/usr/bin/tini", "--", "/x"])).ok(),
            Some(Stage2 {
                uid: 1000,
                gid: 1000,
                argv: args(&["/usr/bin/tini", "--", "/x"])
            })
        );
        for bad in [
            &["0", "1000", "--", "/x"][..],
            &["1000", "0", "--", "/x"],
            &["1000", "1000", "--"],
            &["1000", "1000", "/x"],
            &["a", "1", "--", "/x"],
            &[],
        ] {
            assert!(Stage2::parse(&args(bad)).is_err(), "{bad:?}");
        }
    }
}
