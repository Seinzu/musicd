use std::io::{self, Write};
use std::net::TcpStream;

pub(crate) fn write_sse_event(writer: &mut TcpStream, event: &str, data: &str) -> io::Result<()> {
    write!(writer, "event: {event}\r\n")?;
    for line in data.lines() {
        write!(writer, "data: {line}\r\n")?;
    }
    write!(writer, "\r\n")?;
    writer.flush()
}

pub(crate) fn write_sse_comment(writer: &mut TcpStream, comment: &str) -> io::Result<()> {
    write!(writer, ": {comment}\r\n\r\n")?;
    writer.flush()
}
