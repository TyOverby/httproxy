use std::io;
use std::net;
use std::thread;
use std::io::Result as IoResult;
use std::net::Shutdown;
use std::ascii::AsciiExt;

#[derive(Debug)]
enum RequestType {
    Regular,
    Connect
}

// Returns the request type and the host, if they can be found
fn gather_info<'a, I: Iterator<Item = &'a str>>(headers: I) -> Option<(RequestType, String, u16)> {
    let mut headers = headers.map(AsciiExt::to_ascii_lowercase);
    let request_type = headers.next()
        .map(|first| {
            println!(">>> {}", first);
            if first.contains("connect") {
                RequestType::Connect
            } else {
                RequestType::Regular
            }
        });

    let mut host: Option<String> = None;
    let mut port: Option<u16> = None;

    for header in headers {
        if header.starts_with("host: ") {
            if let Some(h) = header.splitn(2, ":").nth(1).map(|s| s.trim()) {
                let mut parts = h.rsplitn(2, ":");
                match (parts.next(), parts.next()) {
                    (Some(host_part), None) => {
                        host = Some(host_part.to_string());
                    }
                    (Some(port_part), Some(host_part)) => {
                        host = Some(host_part.to_string());
                        port = port_part.parse().ok()
                    }
                    _ => return None
                }
            }
            break;
        }
    }

    match (request_type, host, port) {
        (Some(req), Some(host), Some(port)) => Some((req, host, port)),
        (Some(req), Some(host), None)       => Some((req, host, 80)),
        _ => None
    }
}

// Called once per connection on its own thread.
fn handle_connection(mut tcp_stream: net::TcpStream) -> IoResult<()> {
    use std::io::{BufRead, Read, Write};

    let header_lines: IoResult<Vec<_>> =
        io::BufReader::new(Read::by_ref(&mut tcp_stream))
            // Go through all the lines
            .lines()
            // Take all the ones before \r\n that don't error out
            .take_while(|s| s.as_ref().map(|s| s != "\r").unwrap_or(false))
            // group them into an IoResult<Vec<_>>
            .collect();
    // Fail if any io error occurred since the beginning
    let header_lines = try!(header_lines);

    // Gather information from the header lines
    if let Some((req, host, port)) = gather_info(header_lines.iter().map(|s| &s[..])) {
        let mut to_server = try!(net::TcpStream::connect((&host[..], port)));
        match req {
            RequestType::Regular => {
                for line in header_lines {
                    try!(write!(to_server, "{}\r\n", line));
                }
                try!(write!(to_server, "\r\n"));
            }
            RequestType::Connect => {
                try!(write!(tcp_stream, "HTTP/1.1 200\r\n\r\n"));
            }
        }

        let mut c_copy = try!(tcp_stream.try_clone());
        let mut s_copy = try!(to_server.try_clone());

        let j1 = thread::spawn(move || {
            let r = io::copy(&mut tcp_stream, &mut to_server);
            let s1r = tcp_stream.shutdown(Shutdown::Read);
            let s2r = to_server.shutdown(Shutdown::Write);
            r.map(|_|()).or(s1r).or(s2r)
        });

        let j2 = thread::spawn(move || {
            let r = io::copy(&mut s_copy, &mut c_copy);
            let s1r = s_copy.shutdown(Shutdown::Read);
            let s2r = c_copy.shutdown(Shutdown::Write);
            r.map(|_|()).or(s1r).or(s2r)
        });

        try!(j1.join().unwrap_or(Ok(())));
        try!(j2.join().unwrap_or(Ok(())));
    }

    Ok(())
}

fn main() {
    let first_arg: Option<u16> = std::env::args().nth(1).and_then(|a| a.parse().ok());
    let port = first_arg.expect("Expected command line argument as a port");

    let listener = net::TcpListener::bind(("127.0.0.1", port));
    let listener = listener.ok().expect("Expected port to be open");

    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            thread::spawn(move || {
                let res = handle_connection(stream);
                if let Err(e) = res {
                    println!("ERROR: {}", e);
                }
            });
        }
    }
}
