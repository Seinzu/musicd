use std::io::{self, Write};

use super::ResponseWriter;

pub(crate) fn write_sse_event(
    writer: &mut ResponseWriter,
    event: &str,
    data: &str,
) -> io::Result<()> {
    write!(writer, "event: {event}\r\n")?;
    for line in data.lines() {
        write!(writer, "data: {line}\r\n")?;
    }
    write!(writer, "\r\n")?;
    writer.flush()
}

pub(crate) fn write_sse_comment(writer: &mut ResponseWriter, comment: &str) -> io::Result<()> {
    write!(writer, ": {comment}\r\n\r\n")?;
    writer.flush()
}
