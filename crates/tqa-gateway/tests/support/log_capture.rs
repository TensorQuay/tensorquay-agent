use std::{
    io,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub struct Capture(Arc<Mutex<Vec<u8>>>);
impl Capture {
    pub fn text(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(String::from_utf8(
            self.0.lock().map_err(|_| "capture lock")?.clone(),
        )?)
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
impl io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut buffer = self
            .0
            .lock()
            .map_err(|_| io::Error::other("capture lock"))?;
        if buffer.len() + bytes.len() > 1024 * 1024 {
            return Err(io::Error::other("capture bound"));
        }
        buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
