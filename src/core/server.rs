use colored::*;
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use crate::protocol::request::{HttpMethod, Request};
use crate::protocol::response::Response;
use crate::routing::trie::{RequestContext, Router, RoutingResult};
use crate::security::middleware::MiddlewareResult;
use crate::security::xss::{SafeHtml, Sanitizer, UntrustedString};

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

pub fn run_server(host: &str, port: &str, router: Router) {
    let listener = TcpListener::bind(format!("{}:{}", host, port)).unwrap();
    let pool = ThreadPool::new(4);

    println!(
        "{} {}:{}",
        "Security Kernel Online at".green().bold(),
        host,
        port
    );

    // Wrap in an Arc so multiple threads in ThreadPool can read it safely
    let shared_router = std::sync::Arc::new(router);

    for stream in listener.incoming() {
        let stream = stream.unwrap();
        let router_ptr = std::sync::Arc::clone(&shared_router);

        pool.execute(move || {
            handle_connection(stream, &router_ptr);
        });
    }
}

fn handle_connection(mut stream: TcpStream, router: &Router) {
    if let Err(e) = stream.set_read_timeout(Some(Duration::from_secs(5))) {
        eprintln!("Kernel Error: Failed to set read timeout: {}", e);
        return;
    }

    if let Err(e) = stream.set_write_timeout(Some(Duration::from_secs(5))) {
        eprintln!("Kernel Error: Failed to set write timeout: {}", e);
        return;
    }

    match Request::parse(&mut stream) {
        Ok(req) => {
            println!("Request Received: {:?} {}", req.method, req.path);
            println!("body {:?}", req.body.len());

            match router.run_middlewares(&req) {
                MiddlewareResult::Error(err_res) => {
                    let _ = stream.write_all(&err_res.to_bytes());
                    return;
                }

                MiddlewareResult::Next => {
                    let routing_result = router.match_route(&req.method, &req.path);

                    let response = match routing_result {
                        RoutingResult::Found(handler, ctx) => {
                            let body: SafeHtml = handler(ctx);
                            Response::new(200, body)
                        }
                        RoutingResult::NotFound => {
                            Response::new(404, Sanitizer::trust("<h1>404 Not Found</h1>"))
                        }
                        RoutingResult::MethodNotAllowed => {
                            Response::new(405, Sanitizer::trust("<h1>405 Method Not Allowed</h1>"))
                        }
                    };

                    // Write Secure Response
                    let _ = stream.write_all(&response.to_bytes());
                }
            }
        }

        Err(e) => {
            eprintln!("{} {}", "Security Warning:".red().bold(), e);

            let err_body = Sanitizer::trust("<h1>Bad Request</h1>");
            let response = Response::new(400, err_body);
            let _ = stream.write_all(&response.to_bytes());
        }
    }
}
