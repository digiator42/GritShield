#![no_main]

use libfuzzer_sys::fuzz_target;
use gritshield::protocol::request::Request;

fuzz_target!(|data: &[u8]| {
    let _ = std::str::from_utf8(data);
});