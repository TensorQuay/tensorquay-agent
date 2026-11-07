//! The per-VM nftables ruleset (SBX-F-014), loaded by the launcher inside the VM's own container network namespace.
//! The guest may open TCP connections to the gateway's address on the configured ports and nothing else: no UDP, no
//! DNS, no other address, and nothing addressed to the container itself. The container's internal network is the
//! second layer: it has no route out at all (ADR-0021).

use std::{fmt::Write, net::Ipv4Addr};

/// The tap's address in the container; identical in every VM because each has a private network namespace.
pub(crate) const TAP_ADDRESS: Ipv4Addr = Ipv4Addr::new(169, 254, 100, 1);
/// The guest's address on the tap link.
pub(crate) const GUEST_ADDRESS: Ipv4Addr = Ipv4Addr::new(169, 254, 100, 2);
/// The tap device's name inside the container.
pub(crate) const TAP: &str = "tap0";

/// The kernel command-line network setting for the guest: static address, tap as gateway, no autoconfiguration.
pub(crate) fn kernel_ip_argument() -> String {
    format!("ip={GUEST_ADDRESS}::{TAP_ADDRESS}:255.255.255.252::eth0:off")
}

/// The ruleset for one VM. `ports` are validated by the backend configuration (1 through 8, distinct, non-zero).
pub(crate) fn ruleset(gateway: Ipv4Addr, ports: &[u16]) -> String {
    let ports = ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut text = String::new();
    let _ = write!(
        text,
        "table inet tqa {{
\tchain input {{
\t\ttype filter hook input priority filter; policy accept;
\t\tiifname \"{TAP}\" drop
\t}}
\tchain forward {{
\t\ttype filter hook forward priority filter; policy drop;
\t\tct state established,related accept
\t\tiifname \"{TAP}\" oifname \"eth0\" ip saddr {GUEST_ADDRESS} ip daddr {gateway} tcp dport {{ {ports} }} ct state new accept
\t}}
\tchain postrouting {{
\t\ttype nat hook postrouting priority srcnat; policy accept;
\t\toifname \"eth0\" ip saddr {GUEST_ADDRESS} masquerade
\t}}
}}
"
    );
    text
}

#[cfg(test)]
mod tests {
    use std::{
        error::Error,
        io::Write,
        net::Ipv4Addr,
        process::{Command, Stdio},
    };

    use super::{kernel_ip_argument, ruleset};

    const GOLDEN: &str = "table inet tqa {
\tchain input {
\t\ttype filter hook input priority filter; policy accept;
\t\tiifname \"tap0\" drop
\t}
\tchain forward {
\t\ttype filter hook forward priority filter; policy drop;
\t\tct state established,related accept
\t\tiifname \"tap0\" oifname \"eth0\" ip saddr 169.254.100.2 ip daddr 192.0.2.10 tcp dport { 8080, 3128 } ct state new accept
\t}
\tchain postrouting {
\t\ttype nat hook postrouting priority srcnat; policy accept;
\t\toifname \"eth0\" ip saddr 169.254.100.2 masquerade
\t}
}
";

    #[test]
    fn sbx_f_014_ruleset_golden() {
        let rules = ruleset(Ipv4Addr::new(192, 0, 2, 10), &[8080, 3128]);
        assert_eq!(rules, GOLDEN);
        assert!(
            !rules.contains("udp") && !rules.contains(" 53"),
            "no UDP and no DNS rule"
        );
        assert_eq!(
            rules.matches("accept").count(),
            4,
            "two accepting chain policies and two allow rules only"
        );
        assert_eq!(
            kernel_ip_argument(),
            "ip=169.254.100.2::169.254.100.1:255.255.255.252::eth0:off"
        );
    }

    #[test]
    #[ignore = "docker"]
    fn sbx_f_014_ruleset_loads_in_the_vm_host_image() -> Result<(), Box<dyn Error>> {
        let image = std::env::var("TQA_VMM_IMAGE")
            .map_err(|_| "TQA_VMM_IMAGE must name a built images/vmm image")?;
        let mut child = Command::new("docker")
            .args([
                "run",
                "--rm",
                "-i",
                "--network",
                "none",
                "--user",
                "0:0",
                "--cap-drop",
                "ALL",
            ])
            .args([
                "--cap-add",
                "NET_ADMIN",
                "--entrypoint",
                "nft",
                &image,
                "-c",
                "-f",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or("no stdin")?
            .write_all(ruleset(Ipv4Addr::new(192, 0, 2, 10), &[8080, 3128]).as_bytes())?;
        let output = child.wait_with_output()?;
        assert!(
            output.status.success(),
            "nft rejected the ruleset: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }
}
