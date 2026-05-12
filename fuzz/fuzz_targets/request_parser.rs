#![no_main]

use gritshield::protocol::request::Request;
use libfuzzer_sys::fuzz_target;

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread;

fuzz_target!(|data: &[u8]| {
    // Ignore empty inputs
    if data.is_empty() {
        return;
    }

    // Send fuzzed bytes
    let input = data.to_vec();

    thread::spawn(move || {
        if let Ok((mut socket, _)) = listener.accept() {
            let _ = socket.write_all(&input);
        }
    });

    // Client side
    if let Ok(stream) = TcpStream::connect("127.0.0.1:8000") {
        let _ = Request::parse(&stream);
    }
});
