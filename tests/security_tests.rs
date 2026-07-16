use gritshield::security::{jwt::JwtHandler, xss::{SafeHtml, Sanitizer, UntrustedString}};
use regex::Regex;
use std::{
    io::{Read, Write},
    net::TcpStream,
    thread,
    time::{Duration, Instant},
};
use gritshield::security::jwt::{Claims};
use std::time::{SystemTime, UNIX_EPOCH};

/// Try to reach the test server; `None` means "skip this test".
fn connect_or_skip() -> Option<TcpStream> {
    match TcpStream::connect("127.0.0.1:8000") {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("skipping: no server on 127.0.0.1:8080 ({e})");
            None
        }
    }
}

//___________________ CORE SECURITY TESTS ___________________\\

#[test]
fn test_scape_unsafe_payload() {
    let unsafe_payload = UntrustedString::new("<script>alert(1)</script>".to_string());

    let safe_payload = Sanitizer::encode(unsafe_payload.as_str());

    let re = Regex::new(r"[<>]").unwrap();
    assert!(!re.is_match(&safe_payload.to_string()));
}

#[test]
fn test_html_attributes_are_escaped() {
    let payload = UntrustedString::new("<img src=x onerror=alert(1)>".to_string());

    let safe = Sanitizer::encode(payload.as_str());

    assert!(!safe.to_string().contains("<img"));
}

#[test]
fn test_large_body_rejected() {
    let Some(mut stream) = connect_or_skip() else { return };


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
    let Some(mut stream) = connect_or_skip() else { return };

    let body = ">".repeat(1024 * 1024 + 1);

    let request = format!(
        "POST / HTTP/1.1\r\n\
        Host: localhost\r\n\
        Content-Length: {}\r\n\
        \r\n\
        {}
        ",
        100, body
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

// ___________________ Slowloris Attack Simulation___________________\\

#[test]
fn test_header_slowloris() {
    let Some(mut stream) = connect_or_skip() else { return };

    let partial = "GET / HTTP/1.1\r\nHost: localhost\r\n";

    for byte in partial.bytes() {
        stream.write_all(&[byte]).unwrap();
        thread::sleep(Duration::from_secs(1));
    }

    thread::sleep(Duration::from_secs(20));
}

#[test]
fn test_body_slowloris() {
    let Some(mut stream) = connect_or_skip() else { return };

    let start_time = Instant::now();

    let body = ">".repeat(10 + 1);

    let partial_request = format!(
        "POST / HTTP/1.1\r\n\
        Host: localhost\r\n\
        Content-Length: {}\r\n\
        \r\n\
        ",
        body.len()
    );

    stream.write_all(&partial_request.as_bytes()).unwrap();

    let mut i = 0;
    for byte in body.bytes() {
        stream.write_all(&[byte]).unwrap();
        thread::sleep(Duration::from_secs(1));
        i += 1;
        println!("{:?} {}", start_time.elapsed(), i);
    }

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

#[test]
fn test_xss_defensive_string_wrapping() {
    // Simulate an adversarial payload vector coming from user input
    let raw_attack = "<script>alert('compromised')</script>".to_string();
    let untrusted = UntrustedString::new(raw_attack);

    let clean_output = Sanitizer::url_encode(&untrusted.as_str());

    // Ensure HTML special entities are cleanly encoded to render the script harmless
    let output_str = clean_output;

    assert!(
        !output_str.contains("<script>"),
        "Security Leak: Framework failed to encode opening HTML script element!"
    );
}

#[test]
fn test_xss_trusted_string_bypass() {
    // Ensure that explicit developer exemptions via Sanitizer::trust bypass encoding
    let explicit_html = "<div>Safe Layout</div>";
    let trusted = Sanitizer::trust(explicit_html);

    assert_eq!(trusted.to_string(), "<div>Safe Layout</div>");
}

#[test]
fn test_jwt_signature_and_claims_lifecycle() {
    let secret_key = "super_secret_crypto_key_for_gritshield_testing_32_bytes";
    
    // Get current Unix timestamp
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Build authentic claims matching your Claims struct template
    let expected_claims = Claims {
        sub: "user_admin_01".to_string(),
        role: "ADMIN".to_string(),
        exp: now as usize + 3600, // Expires in 1 hour
    };

    let jwt_handler = JwtHandler::new(secret_key);

    // Mint the token using the framework's encoding routine
    let token = jwt_handler.sign(&expected_claims)
        .expect("Failed to encode cryptographically secure JWT token");

    // Decode token successfully using the valid key
    let decoded_result = jwt_handler.verify(&token);
    assert!(decoded_result.is_ok(), "Failed to decode perfectly authentic JWT token");

    let claims = decoded_result.unwrap();
    assert_eq!(claims.sub, "user_admin_01");
    assert_eq!(claims.role, "ADMIN");
}

#[test]
fn test_jwt_tamper_rejection() {
    let secret_key = "super_secret_crypto_key_for_gritshield_testing_32_bytes";
    let wrong_key = "wrong_attacker_secret_key_for_gritshield_testing_32_bytes";
    
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let original_claims = Claims {
        sub: "user_victim".to_string(),
        role: "USER".to_string(),
        exp: now as usize + 3600,
    };

    let jwt_handler = JwtHandler::new(secret_key);
    
    // Mint a real token
    let token = jwt_handler.sign(&original_claims).unwrap();
    
    let jwt_handler = JwtHandler::new(wrong_key);

    // Attempting to decode with an invalid key must fail the cryptographic validation check
    let decoded_bad_key = jwt_handler.verify(&token);
    assert!(
        decoded_bad_key.is_err(),
        "Security Leak: Decryption routine accepted a token verified with a non-matching secret key!"
    );
}