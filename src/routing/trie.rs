use std::collections::HashMap;

type Handler = fn() -> String;

pub struct Node {
    pub childern: HashMap<String, Node>,
    pub is_end: bool,
    pub handler: Option<Handler>,
    pub parameter_name: Option<String>,
}

impl Node {
    pub fn new() -> Self {
        Node {
            childern: HashMap::new(),
            is_end: false,
            handler: None,
            parameter_name: None,
        }
    }
}

pub struct Router {
    root: Node,
}

impl Router {
    pub fn new() -> Self {
        Router { root: Node::new() }
    }

    pub fn add_route(&mut self, path: &str, handler: Handler) {
        let mut current = &mut self.root;

        for segment in path.split('/').filter(|s| !s.is_empty()) {
            if segment.starts_with(':') {
                let param_name = segment[1..].to_string();
                current = current
                    .childern
                    .entry(":param".to_string())
                    .or_insert(Node::new());
                current.parameter_name = Some(param_name);
            }

            current = current
                .childern
                .entry(segment.to_string())
                .or_insert(Node::new());
        }
        current.is_end = true;
        current.handler = Some(handler);
    }

    pub fn match_route(&self, path: &str) -> Option<(Handler, HashMap<String, String>)> {
        let mut current = &self.root;
        let mut params = HashMap::new();

        for segment in path.split('/').filter(|s| !s.is_empty()) {
            if let Some(next_node) = current.childern.get(segment) {
                current = next_node;
            } else if let Some(param_node) = current.childern.get(":param") {
                if let Some(ref name) = param_node.parameter_name {
                    // If the segment isn't a direct match, check if this level accepts a parameter
                    params.insert(name.clone(), segment.to_string());
                }
                current = param_node;
            } else {
                return None;
            }
        }
        current.handler.map(|h| (h, params))
    }
}
