use std::io::prelude::*;
use std::net::{Shutdown, TcpListener};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use crate::protocol::request::Request;

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: mpsc::Sender<Job>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

impl ThreadPool {
    pub fn new(size: usize) -> ThreadPool {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel();

        let reciever = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&reciever)));
        }

        ThreadPool { workers, sender }
    }

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.send(job).unwrap();
    }
}

struct Worker {
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    pub fn new(id: usize, reciever: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || {
            loop {
                let job = reciever.lock().unwrap().recv().unwrap();
                job();
            }
        });

        Worker {
            id,
            thread: Some(thread),
        }
    }
}

pub fn run_server(host: &str, port: &str) {
    let listener = TcpListener::bind(format!("{}:{}", host, port)).unwrap();
    let pool = ThreadPool::new(4);

    println!("Security Kernel Online at {}:{}", host, port);

    for stream in listener.incoming() {
        let stream = stream.unwrap();

        pool.execute(|| {
            handle_connection(stream);
        });
    }
}

fn handle_connection(mut stream: std::net::TcpStream) {
    if let Err(e) = stream.set_read_timeout(Some(Duration::from_secs(5))) {
        eprintln!("Kernel Error: Failed to set read timeout: {}", e);
        return;
    }

    if let Err(e) = stream.set_write_timeout(Some(Duration::from_secs(5))) {
        eprintln!("Kernel Error: Failed to set write timeout: {}", e);
        return;
    }

    match Request::parse(&mut stream) {
        Ok(request) => {
            println!("Request Received: {:?} {}", request.method, request.path);
            println!("body {:?}", request.body.len());

            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nKernerl Verified.";
            stream.write_all(response.as_bytes()).unwrap();
        }

        Err(e) => {
            eprintln!("Security Warning: {}", e);

            let response = "HTTP/1.1 400 Bad Request\r\n\r\nInvalid Syntax.";
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();

            let _ = stream.shutdown(Shutdown::Write);
        }
    }
}
