use super::*;

#[test]
fn tk_u_005_host_facts_parse_and_absent_facts_fail_validation() {
    let cpuinfo = "processor\t: 0\nmodel name\t: Example CPU 9000\nprocessor\t: 1\nmodel name\t: Example CPU 9000\n";
    assert_eq!(cpu_model(cpuinfo), "Example CPU 9000");
    assert_eq!(
        memory_bytes("MemTotal:       65536 kB\nMemFree: 1 kB\n"),
        65536 * 1024
    );
    // Missing facts come back empty or zero, which the result writer refuses (TK-U-005).
    assert_eq!(
        (cpu_model("processor: 0\n"), memory_bytes("MemFree: 1 kB\n")),
        (String::new(), 0)
    );
    assert_eq!(memory_bytes("MemTotal: many kB\n"), 0);
}
