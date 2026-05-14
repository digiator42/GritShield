use colored::*;
use std::collections::HashMap;
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use crate::protocol::request::{HttpMethod, Request};
use crate::protocol::response::{Response, ResponseBody};
use crate::routing::trie::{RequestContext, Router, RoutingResult};
use crate::security::middleware::MiddlewareResult;
use crate::security::session::{Session, SessionStore};
use crate::security::xss::{SafeHtml, Sanitizer, UntrustedString};
use crate::utils::reloader::HotReloader;

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

pub fn run_server(host: &str, port: &str, router: Router, use_reloader: bool) {
    if use_reloader {
        // If we are the supervisor, this function will block and run the watcher loop.
        // If we are the child worker, it returns immediately and boots the actual TCP listener.
        HotReloader::start();

        if std::env::var("RUNNING_UNDER_RELOADER").is_err() {
            // The supervisor exits here once the main watcher loop closes
            std::process::exit(0);
        }
    }

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
            // Run Middleware Chain
            match router.run_middlewares(&req) {
                MiddlewareResult::Error(err_res) => {
                    let (bytes, mime) = err_res.resolve();
                    let _ = stream.write_all(&err_res.to_bytes(&bytes, &mime));
                    return;
                }
                MiddlewareResult::Next(session_state) => {
                    // Capture the carried state
                    let routing_result = router.match_route(&req.method, &req.path);

                    let response = match routing_result {
                        RoutingResult::Found(handler, params) => {
                            // Extract session data carried from middleware
                            let (session_ptr, is_new_session) = match session_state {
                                Some((ptr, is_new)) => (Some(ptr), is_new),
                                None => (None, false),
                            };

                            let form = req.parse_form_body();

                            let ctx = RequestContext {
                                params,
                                headers: req.headers.clone(),
                                claims: None,
                                query: req.query.clone(),
                                session: session_ptr.clone(),
                                form,
                            };

                            let mut res = handler(ctx);

                            // Set the cookie only if the middleware flagged it as new
                            if is_new_session {
                                if let Some(s) = session_ptr {
                                    let sid = s.lock().unwrap().id.clone();
                                    res.cookies.push(crate::protocol::response::Cookie::new(
                                        "session_id",
                                        &sid,
                                    ));
                                }
                            }
                            res
                        }
                        RoutingResult::NotFound => {
                            Response::new(404, Sanitizer::trust("<h1>404</h1>"))
                        }
                        RoutingResult::MethodNotAllowed => {
                            Response::new(405, Sanitizer::trust("<h1>405</h1>"))
                        }
                    };

                    let (bytes, mime) = response.resolve();
                    let _ = stream.write_all(&response.to_bytes(&bytes, &mime));
                }
            }
        }

        Err(e) => {
            eprintln!("{} {}", "Security Warning:".red().bold(), e);
            let err_res = Response::new(400, Sanitizer::trust("<h1>Bad Request</h1>"));
            let (bytes, mime) = err_res.resolve();
            let _ = stream.write_all(&err_res.to_bytes(&bytes, &mime));
        }
    }
}
