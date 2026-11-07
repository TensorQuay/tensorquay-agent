//! RT-I-054's host half: a display's clients told strictly, by a socket connected to the display's own, over this
//! test's own listener (it plays the X server) and real processes; and the socket diagnostics' answer read as sent.
use super::{HEADER, PEER, answers, clients, holds, linked, listening, request};
use std::{
    collections::HashMap,
    io::Read as _,
    os::unix::net::UnixListener,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A child that kills itself with the test: whatever happens, nothing is left running.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn pid(child: &Running) -> Result<i32, Box<dyn std::error::Error>> {
    Ok(i32::try_from(child.0.id())?)
}

// TEST: RT-I-054
#[test]
fn rt_i_054_a_process_connected_to_the_display_is_its_client_and_a_plain_one_is_not() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let display = directory.path().join("X0");
    let listener = UnixListener::bind(&display)?;
    // An application: connected to the display's socket, and nothing to do with DISPLAY.
    let mut application = Running(
        Command::new("python3")
            .args([
                "-c",
                "import socket, sys, time\ns = socket.socket(socket.AF_UNIX)\ns.connect(sys.argv[1])\nprint('in', flush=True)\ntime.sleep(60)",
                display.to_str().ok_or("path")?,
            ])
            .stdout(Stdio::piped())
            .spawn()?,
    );
    // A plain background process, given the display's name in its environment and nothing else.
    let plain = Running(
        Command::new("sleep")
            .arg("60")
            .env("DISPLAY", ":0")
            .spawn()?,
    );
    let (_accepted, _) = listener.accept()?;
    let mut said = [0_u8; 3];
    application
        .0
        .stdout
        .as_mut()
        .ok_or("stdout")?
        .read_exact(&mut said)?;
    let started = Instant::now();
    let mut found = clients(&display, 4096);
    while found.is_empty() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
        found = clients(&display, 4096);
    }
    assert!(
        !found.is_empty(),
        "the display's accepted connection had no peer"
    );
    assert!(
        holds(pid(&application)?, &found),
        "the connected process isn't a client"
    );
    assert!(
        !holds(pid(&plain)?, &found),
        "a process with DISPLAY alone is a client"
    );
    // Another path is another display: none of its clients.
    assert!(clients(&directory.path().join("X1"), 4096).is_empty());
    Ok(())
}

// TEST: RT-I-054
#[test]
fn rt_i_054_the_dump_request_and_its_answer_are_read_as_the_kernel_writes_them() -> TestResult {
    let sent = request();
    assert_eq!(sent.len(), 40);
    assert_eq!(u32::from_ne_bytes(sent[0..4].try_into()?), 40);
    assert_eq!(
        u16::from_ne_bytes(sent[4..6].try_into()?),
        20,
        "SOCK_DIAG_BY_FAMILY"
    );
    assert_eq!(sent[HEADER], 1, "AF_UNIX");
    // One unix_diag_msg (inode 7) with its peer (inode 9), then the dump's end.
    let mut message = vec![1, 1, 1, 0];
    message.extend_from_slice(&7_u32.to_ne_bytes());
    message.extend_from_slice(&[0; 8]);
    message.extend_from_slice(&8_u16.to_ne_bytes());
    message.extend_from_slice(&PEER.to_ne_bytes());
    message.extend_from_slice(&9_u32.to_ne_bytes());
    let mut answer = Vec::new();
    answer.extend_from_slice(&u32::try_from(HEADER + message.len())?.to_ne_bytes());
    answer.extend_from_slice(&20_u16.to_ne_bytes());
    answer.extend_from_slice(&[0; 10]);
    answer.extend_from_slice(&message);
    let mut found = HashMap::new();
    assert_eq!(answers(&answer, &mut found), Some(true), "the dump goes on");
    assert_eq!(found.get(&7), Some(&9));
    let mut done = Vec::new();
    done.extend_from_slice(&20_u32.to_ne_bytes());
    done.extend_from_slice(&3_u16.to_ne_bytes());
    done.extend_from_slice(&[0; 14]);
    assert_eq!(answers(&done, &mut found), Some(false), "the dump's end");
    // A message with no peer gives none; an attribute shorter than its header ends the reading.
    assert_eq!(linked(&message[..16]), None);
    let mut broken = message[..16].to_vec();
    broken.extend_from_slice(&[2, 0, 2, 0]);
    assert_eq!(linked(&broken), None);
    Ok(())
}

// TEST: RT-I-054
#[test]
fn rt_i_054_the_display_s_listener_is_told_from_the_connections_listed_under_its_path() {
    // The kernel lists an accepted connection under its listener's path: only the listening flag tells them apart.
    let table = "Num       RefCount Protocol Flags    Type St Inode Path
0000000000000000: 00000002 00000000 00010000 0001 01 101 /tmp/.X11-unix/X0
0000000000000000: 00000003 00000000 00000000 0001 03 102 /tmp/.X11-unix/X0
0000000000000000: 00000002 00000000 00010000 0001 01 103 @/tmp/.X11-unix/X0
0000000000000000: 00000002 00000000 00010000 0001 01 104 /tmp/.X11-unix/X1
0000000000000000: 00000003 00000000 00000000 0001 03 105
";
    assert_eq!(
        listening(table, "/tmp/.X11-unix/X0"),
        std::iter::once(101).collect()
    );
    assert!(listening(table, "/tmp/.X11-unix/X2").is_empty());
}
