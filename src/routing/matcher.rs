use crate::http::request::HttpMethod;
use crate::routing::engine::RequestContext;
use std::sync::Arc;

/// One parsed segment of an Ant-style path pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    /// A literal segment such as `users`.
    Literal(String),
    /// `*` — exactly one non-empty segment.
    Wildcard,
    /// `:id` — exactly one non-empty segment, captured by the router.
    Named(String),
    /// `**` — zero or more trailing segments. Only valid as the last segment.
    CatchAll,
}

/// An Ant-style path pattern.
///
/// Supported syntax:
///
/// | Pattern | Matches |
/// |---|---|
/// | `/users/:id` | `/users/42`, not `/users/42/posts` |
/// | `/files/*` | `/files/logo.png`, not `/files/a/b` |
/// | `/api/**` | `/api`, `/api/users`, `/api/users/42` |
///
/// A trailing `:*` is accepted as an alias for `**`, matching the historical
/// `AuthMiddleware` public-path syntax. `**` anywhere other than the last
/// segment is treated as a literal.
#[derive(Debug, Clone)]
pub struct PathPattern {
    raw: String,
    segments: Vec<Segment>,
}

impl PathPattern {
    pub fn new(pattern: impl Into<String>) -> Self {
        let raw = pattern.into();
        let raw_segments: Vec<&str> = raw
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();

        let last_index = raw_segments.len().saturating_sub(1);
        let segments = raw_segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                if index == last_index && (*segment == "**" || segment.starts_with(":*")) {
                    Segment::CatchAll
                } else if *segment == "*" {
                    Segment::Wildcard
                } else if segment.len() > 1 && segment.starts_with(':') {
                    Segment::Named(segment[1..].to_string())
                } else {
                    Segment::Literal((*segment).to_string())
                }
            })
            .collect();

        Self { raw, segments }
    }

    /// The pattern exactly as it was written.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// True when `path` is covered by this pattern.
    pub fn matches(&self, path: &str) -> bool {
        let path_segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();

        let mut consumed = 0;

        for segment in &self.segments {
            match segment {
                Segment::CatchAll => return true,
                Segment::Literal(expected) => {
                    if path_segments.get(consumed) != Some(&expected.as_str()) {
                        return false;
                    }
                    consumed += 1;
                }
                Segment::Wildcard | Segment::Named(_) => {
                    if consumed >= path_segments.len() {
                        return false;
                    }
                    consumed += 1;
                }
            }
        }

        consumed == path_segments.len()
    }
}

/// Convenience wrapper for one-shot checks.
pub fn path_matches(pattern: &str, path: &str) -> bool {
    PathPattern::new(pattern).matches(path)
}

/// Which HTTP methods a [`RequestMatcher`] accepts.
#[derive(Debug, Clone, Default)]
pub enum MethodMatcher {
    /// Every method, including `UNKNOWN`.
    #[default]
    Any,
    /// Exactly one method.
    Only(HttpMethod),
    /// Any of the listed methods. An empty list matches nothing.
    AnyOf(Vec<HttpMethod>),
}

impl MethodMatcher {
    pub fn matches(&self, method: &HttpMethod) -> bool {
        match self {
            MethodMatcher::Any => true,
            MethodMatcher::Only(expected) => expected == method,
            MethodMatcher::AnyOf(methods) => methods.contains(method),
        }
    }
}

type RequestPredicate = Arc<dyn Fn(&RequestContext) -> bool + Send + Sync>;

/// Decides whether a security chain, a filter, or an authorization rule applies
/// to a request.
///
/// A matcher with no path constraint matches every path; with no method
/// constraint it matches every method; headers and predicates are additive.
#[derive(Clone)]
pub struct RequestMatcher {
    path: Option<PathPattern>,
    methods: MethodMatcher,
    headers: Vec<(String, String)>,
    predicate: Option<RequestPredicate>,
}

impl std::fmt::Debug for RequestMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestMatcher")
            .field("path", &self.path)
            .field("methods", &self.methods)
            .field("headers", &self.headers)
            .field("has_predicate", &self.predicate.is_some())
            .finish()
    }
}

impl Default for RequestMatcher {
    fn default() -> Self {
        Self::any()
    }
}

impl RequestMatcher {
    /// Matches every request.
    pub fn any() -> Self {
        Self {
            path: None,
            methods: MethodMatcher::Any,
            headers: Vec::new(),
            predicate: None,
        }
    }

    /// Matches a path pattern.
    pub fn path(pattern: impl Into<String>) -> Self {
        Self::any().and_path(pattern)
    }

    /// Matches a path pattern for exactly one HTTP method.
    pub fn path_and_method(pattern: impl Into<String>, method: HttpMethod) -> Self {
        Self::path(pattern).method(method)
    }

    pub fn and_path(mut self, pattern: impl Into<String>) -> Self {
        self.path = Some(PathPattern::new(pattern));
        self
    }

    /// Restrict the matcher to one HTTP method.
    pub fn method(mut self, method: HttpMethod) -> Self {
        self.methods = MethodMatcher::Only(method);
        self
    }

    /// Restrict the matcher to any of `methods`.
    pub fn methods<I>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = HttpMethod>,
    {
        self.methods = MethodMatcher::AnyOf(methods.into_iter().collect());
        self
    }

    /// Require an exact request header value. The name is case-insensitive.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .push((name.into().to_ascii_lowercase(), value.into()));
        self
    }

    /// Add an arbitrary predicate; it runs after every other constraint.
    pub fn when<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&RequestContext) -> bool + Send + Sync + 'static,
    {
        self.predicate = Some(Arc::new(predicate));
        self
    }

    pub fn matches(&self, ctx: &RequestContext) -> bool {
        if let Some(path) = &self.path {
            if !path.matches(&ctx.req.path) {
                return false;
            }
        }

        if !self.methods.matches(&ctx.req.method) {
            return false;
        }

        for (name, expected) in &self.headers {
            match ctx.header(name) {
                Some(actual) if actual == expected => {}
                _ => return false,
            }
        }

        match &self.predicate {
            Some(predicate) => predicate(ctx),
            None => true,
        }
    }
}

impl From<&str> for RequestMatcher {
    fn from(pattern: &str) -> Self {
        Self::path(pattern)
    }
}

impl From<String> for RequestMatcher {
    fn from(pattern: String) -> Self {
        Self::path(pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_root_patterns() {
        assert!(PathPattern::new("/").matches("/"));
        assert!(!PathPattern::new("/").matches("/users"));
        assert!(PathPattern::new("/users").matches("/users/"));
        assert!(!PathPattern::new("/users").matches("/users/42"));
    }

    #[test]
    fn single_segment_wildcards() {
        assert!(PathPattern::new("/files/*").matches("/files/logo.png"));
        assert!(!PathPattern::new("/files/*").matches("/files/a/b"));
        assert!(PathPattern::new("/users/:id").matches("/users/42"));
        assert!(!PathPattern::new("/users/:id").matches("/users/42/posts"));
    }

    #[test]
    fn catch_all_patterns() {
        assert!(PathPattern::new("/api/**").matches("/api"));
        assert!(PathPattern::new("/api/**").matches("/api/users"));
        assert!(PathPattern::new("/api/**").matches("/api/users/42"));
        assert!(!PathPattern::new("/api/**").matches("/apix"));
        assert!(PathPattern::new("/api/:*").matches("/api/users"));
    }

    #[test]
    fn non_trailing_double_star_is_literal() {
        assert!(PathPattern::new("/a/**/b").matches("/a/**/b"));
        assert!(!PathPattern::new("/a/**/b").matches("/a/x/b"));
    }

    #[test]
    fn method_matcher_respects_constraints() {
        assert!(MethodMatcher::Any.matches(&HttpMethod::GET));
        assert!(MethodMatcher::Only(HttpMethod::POST).matches(&HttpMethod::POST));
        assert!(!MethodMatcher::Only(HttpMethod::POST).matches(&HttpMethod::GET));
        assert!(
            MethodMatcher::AnyOf(vec![HttpMethod::GET, HttpMethod::HEAD])
                .matches(&HttpMethod::HEAD)
        );
        assert!(!MethodMatcher::AnyOf(vec![]).matches(&HttpMethod::GET));
    }
}
