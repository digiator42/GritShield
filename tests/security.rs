use gritshield::security::xss::{SafeHtml, Sanitizer, UntrustedString};
use std::{
    io::{Read, Write},
    net::TcpStream,
};

#[test]
fn test_scape_unsafe_payload() {
    let unsafe_payload = UntrustedString::new("<script>alert(1)</script>".to_string());

    let safe_payload = Sanitizer::encode(unsafe_payload);

    assert_eq!(
        safe_payload.to_string(),
        "&lt;script&gt;alert(1)&lt;&#x2F;script&gt;"
    );
}

#[test]
fn test_html_attributes_are_escaped() {
    let payload = UntrustedString::new("<img src=x onerror=alert(1)>".to_string());

    let safe = Sanitizer::encode(payload);

    assert!(!safe.to_string().contains("<img"));
}

#[test]
fn test_large_body_rejected() {
    let mut stream = TcpStream::connect("127.0.0.1:8000").unwrap();

    let body = ">".repeat(1024 * 1024 + 1);

    let request = format!(
        "POST / HTTP/1.1\r\n\
        Host: localhost\r\n\
        Content-Length: {}\r\n\
        \r\n\
        ",
        body.len()
    );

    stream.write_all(request.as_bytes()).unwrap();

    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).unwrap();

    let response = String::from_utf8_lossy(&buffer);

    assert!(response.contains("400 Bad Request"));
}

#[test]
fn test_large_body_terminate_connection() {
    let mut stream = TcpStream::connect("127.0.0.1:8000").unwrap();

    let body = ">".repeat(1024 * 1024 + 1);

    let request = format!(
        "POST / HTTP/1.1\r\n\
        Host: localhost\r\n\
        Content-Length: {}\r\n\
        \r\n\
        {}
        ",
        body.len(),
        body
    );

    stream.write_all(request.as_bytes()).unwrap();

    let mut buffer: Vec<u8> = Vec::new();
    match stream.read_to_end(&mut buffer) {
        Ok(_) => {
            let response = String::from_utf8_lossy(&buffer);
            assert!(response.contains("400 Bad Request"));
        }

        Err(e) => {
            println!("Connection terminated successfully: {}", e);
        }
    }
}
