//! The X selections and the Desktop's x11vnc, for the desktop probe (RT-I-046's clipboard case, RT-I-058): the
//! image's test-only `xsel`, an RFB session to the product's x11vnc and to a positive control, a paste in.
use std::time::{Duration, Instant};

/// x11vnc on the Desktop's display, as the image's init starts it (`-localhost -rfbport 5900`, no password, cut text
/// received only).
pub const VNC: &str = "127.0.0.1:5900";
/// The positive control's x11vnc: the same display, its clipboard flags left at their defaults.
pub const CONTROL_PORT: &str = "5901";

/// Sets an X selection (`--clipboard` or `--primary`) with the image's test-only `xsel`; it keeps serving it.
pub fn set_selection(which: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    let mut child = std::process::Command::new("xsel")
        .args([which, "--input"])
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(text.as_bytes())?;
    child.wait()?;
    Ok(())
}

pub fn selection(which: &str) -> Result<String, Box<dyn std::error::Error>> {
    let out = std::process::Command::new("xsel")
        .args([which, "--output"])
        .output()?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// An RFB 3.8 session with no security, shared, past `ServerInit`, with its encodings set (Raw). It never asks for a
/// framebuffer update, so the only messages the server may send are a bell, a colour map or cut text.
fn rfb(address: &str) -> Result<std::net::TcpStream, Box<dyn std::error::Error>> {
    use std::io::{Read as _, Write as _};
    let mut stream = std::net::TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut version = [0_u8; 12];
    stream.read_exact(&mut version)?;
    assert!(version.starts_with(b"RFB 003."), "{version:?}");
    stream.write_all(b"RFB 003.008\n")?;
    let mut count = [0_u8; 1];
    stream.read_exact(&mut count)?;
    let mut types = vec![0_u8; usize::from(count[0])];
    stream.read_exact(&mut types)?;
    assert!(types.contains(&1), "no-security offered: {types:?}");
    stream.write_all(&[1])?;
    let mut result = [0_u8; 4];
    stream.read_exact(&mut result)?;
    assert_eq!(u32::from_be_bytes(result), 0, "security accepted");
    stream.write_all(&[1])?; // ClientInit: shared
    let mut init = [0_u8; 24];
    stream.read_exact(&mut init)?;
    let name = u32::from_be_bytes([init[20], init[21], init[22], init[23]]);
    let mut name_bytes = vec![0_u8; usize::try_from(name)?];
    stream.read_exact(&mut name_bytes)?;
    // SetEncodings: type 2, one padding byte, one encoding, Raw (0).
    stream.write_all(&[2, 0, 0, 1, 0, 0, 0, 0])?;
    Ok(stream)
}

/// How long a session's selections keep changing for cut text to come: x11vnc 0.9.17 makes the window it owns the
/// selections with about 10 s into a viewer's connection, and sends cut text only from about 15 s (measured in c17's
/// desktop-probe image: set at 10 s and 13 s, nothing; set at 15 s, at once).
const CUT_TEXT_BOUND: Duration = Duration::from_secs(30);

/// Whether one session to `address` receives cut text within [`CUT_TEXT_BOUND`], the selections set anew each second
/// while it is connected: the control must, and the product's x11vnc must not over the same span.
pub fn receives_cut_text(address: &str, tag: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let mut stream = rfb(address)?;
    let deadline = Instant::now() + CUT_TEXT_BOUND;
    let mut round = 0;
    while Instant::now() < deadline {
        set_selection("--clipboard", &format!("clipboard during {tag} {round}"))?;
        set_selection("--primary", &format!("primary during {tag} {round}"))?;
        if cut_text_within(&mut stream, Duration::from_secs(1))? {
            return Ok(true);
        }
        round += 1;
    }
    Ok(false)
}

/// A second x11vnc on the same display with its clipboard flags at their defaults, for the positive control; it ends
/// when dropped.
pub fn control_server() -> Result<std::process::Child, Box<dyn std::error::Error>> {
    let display = std::env::var("DISPLAY")?;
    let child = std::process::Command::new("x11vnc")
        .args([
            "-display",
            &display,
            "-localhost",
            "-rfbport",
            CONTROL_PORT,
            "-shared",
            "-forever",
            "-nopw",
            "-quiet",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let address = format!("127.0.0.1:{CONTROL_PORT}");
    for _ in 0..50 {
        if std::net::TcpStream::connect(&address).is_ok() {
            return Ok(child);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("the control x11vnc didn't start listening".into())
}

/// Reads server messages until `quiet` passes without one; answers whether any was cut text (type 3).
fn cut_text_within(
    stream: &mut std::net::TcpStream,
    quiet: Duration,
) -> Result<bool, Box<dyn std::error::Error>> {
    use std::io::Read as _;
    stream.set_read_timeout(Some(quiet))?;
    loop {
        let mut kind = [0_u8; 1];
        match stream.read(&mut kind) {
            Ok(0) => return Err("the server closed the connection".into()),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(false);
            }
            Err(error) => return Err(error.into()),
        }
        match kind[0] {
            3 => return Ok(true),
            2 => {} // bell
            1 => {
                let mut header = [0_u8; 5];
                stream.read_exact(&mut header)?;
                let colours = u16::from_be_bytes([header[3], header[4]]);
                let mut entries = vec![0_u8; usize::from(colours) * 6];
                stream.read_exact(&mut entries)?;
            }
            other => {
                return Err(
                    format!("unexpected server message {other} without an update request").into(),
                );
            }
        }
    }
}

/// Pastes `text` into the product's x11vnc with a `ClientCutText` (type 6, 3 padding bytes, the length, Latin-1 text)
/// and waits up to 5 s for the X clipboard to hold it.
pub fn paste_in(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    let mut stream = rfb(VNC)?;
    let mut message = vec![6_u8, 0, 0, 0];
    message.extend_from_slice(&u32::try_from(text.len())?.to_be_bytes());
    message.extend_from_slice(text.as_bytes());
    stream.write_all(&message)?;
    for _ in 0..25 {
        std::thread::sleep(Duration::from_millis(200));
        if selection("--clipboard")? == text {
            return Ok(());
        }
    }
    Err(format!(
        "the paste never reached the X clipboard (it holds {:?})",
        selection("--clipboard")?
    )
    .into())
}
