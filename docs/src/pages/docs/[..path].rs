use gritshield::http::request::HttpMethod;
use gritshield::prelude::*;
use lazy_static::lazy_static;
use maud::{html, Markup, PreEscaped};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use regex::Regex;
use std::fs;
use std::path::PathBuf;
use syntect::highlighting::ThemeSet;
use syntect::html::highlighted_html_for_string;
use syntect::parsing::SyntaxSet;

lazy_static! {
    static ref NUM_PREFIX_REGEX: Regex = Regex::new(r"^\d+_").unwrap();
    static ref CLEAN_SLUG_REGEX: Regex = Regex::new(r"^\d+_|_$").unwrap();
}

pub async fn handler(ctx: RequestContext) -> Response {
    // Extract wildcard route parameters safely
    let doc_subpath = match ctx.params.get("*path") {
        Some(untrusted_val) => untrusted_val.as_str(),
        None => "index",
    };

    println!(
        "[DOCS INFRA] Resolving documentation for slug: {}",
        doc_subpath
    );

    // Convert clean URL path to filesystem path with numbered prefixes
    let fs_path = clean_to_fs_path(doc_subpath);

    // Build file path from docs_content directory
    let mut target_file = PathBuf::from("docs_content");
    for segment in fs_path.split('/') {
        target_file.push(segment);
    }

    // Handle directory requests (look for index.md)
    if target_file.is_dir() {
        target_file.push("index.md");
    } else if target_file.extension().is_none() {
        target_file.set_extension("md");
    }

    // Build sidebar navigation from file structure
    let base_path = PathBuf::from("docs_content");
    let sidebar_items = build_sidebar_tree(&base_path, &base_path, 0);
    let sidebar_html = render_sidebar(doc_subpath, &sidebar_items);

    println!("Filesystem path: {}", target_file.display());
    println!("URL slug (clean): {}", doc_subpath);

    // Read and render the markdown file
    let (title, content_html) = match fs::read_to_string(&target_file) {
        Ok(raw_markdown) => {
            let compiled_html = compile_markdown_to_html(&raw_markdown);
            let title = extract_title(&raw_markdown).unwrap_or(doc_subpath.to_string());
            (title, compiled_html)
        }
        Err(_) => {
            let not_found_html = render_not_found_content(doc_subpath, &sidebar_items);
            ("Page Not Found".to_string(), not_found_html)
        }
    };

    // Render the full page with root-level sidebar
    let page_content = render_documentation_layout(doc_subpath, &title, content_html, sidebar_html);

    render!(ctx, "GritShield Documentation", page_content)
}

/// Converts a clean URL path to filesystem path with numbered prefixes
fn clean_to_fs_path(clean_path: &str) -> String {
    if clean_path == "index" {
        return "index".to_string();
    }

    let segments: Vec<&str> = clean_path.split('/').collect();
    let mut fs_segments = Vec::new();

    // Define mapping from clean names to numbered folder names
    let folder_mapping = vec![
        ("getting-started", "01_getting-started"),
        ("response", "02_response"),
        ("security", "02_security"),
        ("routing", "03_routing"),
        ("architecture", "04_architecture"),
        ("database", "05_database"),
        ("websocket", "06_websocket"),
        ("deployment", "07_deployment"),
    ];

    for segment in segments {
        let mapped = folder_mapping
            .iter()
            .find(|(clean, _)| *clean == segment)
            .map(|(_, numbered)| *numbered)
            .unwrap_or(segment);
        fs_segments.push(mapped);
    }

    fs_segments.join("/")
}

/// Converts a filesystem path with numbered prefixes back to clean URL path
fn fs_to_clean_path(fs_path: &str) -> String {
    let segments: Vec<&str> = fs_path.split('/').collect();
    let mut clean_segments = Vec::new();

    for segment in segments {
        let clean = NUM_PREFIX_REGEX.replace_all(segment, "");
        clean_segments.push(clean.to_string());
    }

    clean_segments.join("/")
}

/// Extracts the first H1 from markdown to use as page title
fn extract_title(markdown: &str) -> Option<String> {
    for line in markdown.lines() {
        if line.starts_with("# ") {
            return Some(line.trim_start_matches("# ").trim().to_string());
        }
    }
    None
}

/// Recursively scans the docs_content folder to build sidebar navigation
fn build_sidebar_tree(
    current_path: &PathBuf,
    base_path: &PathBuf,
    indent: usize,
) -> Vec<(String, String, usize)> {
    let mut items = Vec::new();

    if let Ok(entries) = fs::read_dir(current_path) {
        let mut dirs = Vec::new();
        let mut files = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if path.is_dir() {
                dirs.push((path, name));
            } else if name.ends_with(".md") && name != "index.md" {
                files.push((path, name));
            }
        }

        // Sort directories by name (which works with numbered prefixes)
        dirs.sort_by(|a, b| a.1.cmp(&b.1));
        files.sort_by(|a, b| a.1.cmp(&b.1));

        // Add directories first
        for (dir_path, dir_name) in dirs {
            let relative_path = dir_path.strip_prefix(base_path).unwrap();
            let fs_slug = relative_path.to_string_lossy().replace("\\", "/");

            // Convert filesystem path to clean URL slug
            let clean_slug = fs_to_clean_path(&fs_slug);

            // Remove numeric prefix from display name
            let display_name = NUM_PREFIX_REGEX
                .replace_all(&dir_name, "")
                .replace('_', " ")
                .replace('-', " ");
            let display_name = display_name.trim().to_string();

            items.push((clean_slug, display_name, indent));

            // Recursively add children
            let children = build_sidebar_tree(&dir_path, base_path, indent + 1);
            items.extend(children);
        }

        // Add markdown files (excluding index.md which is handled by the directory)
        for (file_path, file_name) in files {
            let relative_path = file_path.strip_prefix(base_path).unwrap();
            let fs_slug = relative_path
                .to_string_lossy()
                .replace("\\", "/")
                .replace(".md", "");

            // Convert filesystem path to clean URL slug
            let clean_slug = fs_to_clean_path(&fs_slug);

            // Remove numeric prefix from display name
            let display_name = NUM_PREFIX_REGEX
                .replace_all(&file_name.replace(".md", ""), "")
                .replace('_', " ")
                .replace('-', " ");
            let display_name = display_name.trim().to_string();

            items.push((clean_slug, display_name, indent));
        }
    }

    items
}

/// Generates the sidebar HTML from the dynamic file tree using Maud
fn render_sidebar(active_slug: &str, items: &[(String, String, usize)]) -> Markup {
    html! {
        div class="sidebar-header" {
            h3 { "Tree" }
        }
        nav class="docs-sidebar" {
            ul class="sidebar-nav" {
                li class="nav-item" {
                    a href="/docs/index" class={ "nav-link" (if active_slug == "index" { " active" } else { "" }) } {
                        svg class="nav-icon" fill="none" stroke="currentColor" viewBox="0 0 24 24" {
                            path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" {}
                        }
                        span { "Home" }
                    }
                }

                @for (slug, label, indent) in items {
                    @let is_active = active_slug == slug;
                    @let padding_style = format!("padding-left: {}rem;", (*indent as f64 * 1.2) + 0.5);

                    li class="nav-item" style=(padding_style) {
                        a href={ "/docs/" (slug) } class={ "nav-link" (if is_active { " active" } else { "" }) } {
                            svg class="nav-icon" fill="none" stroke="currentColor" viewBox="0 0 24 24" {
                                @if *indent == 0 {
                                    path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" {}
                                } @else {
                                    path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z" {}
                                }
                            }
                            span { (label) }
                        }
                    }
                }
            }
        }
    }
}

/// Compiles raw Markdown text dynamically into semantic HTML markup layout streams
fn compile_markdown_to_html(markdown_input: &str) -> String {
    // Markdown options
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_FOOTNOTES);

    let parser = Parser::new_ext(markdown_input, options);

    // Syntax highlighting setup
    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    let mut html_output = String::new();

    // Walk through events and intercept code blocks
    let mut in_code_block = false;
    let mut code_lang = None;
    let mut code_buffer = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => Some(lang.to_string()),
                    _ => None,
                };
                code_buffer.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                // Highlight code
                if !code_buffer.is_empty() {
                    let syntax = code_lang
                        .as_ref()
                        .and_then(|lang| ps.find_syntax_by_token(lang))
                        .unwrap_or_else(|| ps.find_syntax_plain_text());

                    match highlighted_html_for_string(
                        &code_buffer,
                        &ps,
                        syntax,
                        &ts.themes["base16-ocean.dark"],
                    ) {
                        Ok(highlighted) => {
                            html_output.push_str(&format!(
                                "<div class=\"code-block\">{}</div>",
                                highlighted
                            ));
                        }
                        Err(e) => {
                            error!("Syntax highlighting failed: {}", e);
                            html_output.push_str(&format!(
                                "<pre><code>{}</code></pre>",
                                html_escape::encode_text(&code_buffer)
                            ));
                        }
                    }
                }

                in_code_block = false;
                code_lang = None;
                code_buffer.clear();
            }
            Event::Text(text) if in_code_block => {
                code_buffer.push_str(&text);
            }
            _ => {
                // Default: just push HTML
                pulldown_cmark::html::push_html(&mut html_output, std::iter::once(event));
            }
        }
    }

    html_output
}

/// Renders the complete documentation layout with root-level sidebar
fn render_documentation_layout(
    active_slug: &str,
    title: &str,
    content_html: String,
    sidebar_html: Markup,
) -> Markup {
    html! {
        div class="docs-layout" {
            aside class="docs-sidebar-wrapper" {
                (sidebar_html)
            }

            main class="docs-content-wrapper" {
                article class="docs-article" {
                    header class="docs-header" {
                        div class="breadcrumb" {
                            span { "Documentation" }
                            span class="separator" { "/" }
                            span class="current" {
                                @if active_slug == "index" {
                                    "Home"
                                } @else {
                                    (active_slug)
                                }
                            }
                        }
                        h1 { (title) }
                    }

                    div class="markdown-body" {
                        (PreEscaped(content_html))
                    }
                }
            }
        }
    }
}

/// Renders the 404 content when documentation is not found
fn render_not_found_content(slug: &str, sidebar_items: &[(String, String, usize)]) -> String {
    let available_docs: Vec<String> = sidebar_items.iter().map(|(s, _, _)| s.clone()).collect();

    let mut html_content =
        String::from(r#"<div class="not-found"><h1>Documentation Not Found</h1>"#);
    html_content.push_str(&format!(
        r#"<p>The requested documentation page could not be found.</p>"#
    ));
    html_content.push_str(&format!(
        r#"<div class="not-found-path">Requested: docs_content/{}.md</div>"#,
        slug
    ));

    if !available_docs.is_empty() {
        html_content
            .push_str(r#"<div class="available-docs"><h3>Available Documentation:</h3><ul>"#);
        for doc_slug in available_docs {
            if doc_slug != "index" {
                html_content.push_str(&format!(
                    r#"<li><a href="/docs/{}">{}</a></li>"#,
                    doc_slug, doc_slug
                ));
            }
        }
        html_content.push_str(r#"</ul></div>"#);
    }

    html_content
        .push_str(r#"<a href="/docs" class="back-link">Return to Documentation Home</a></div>"#);
    html_content
}

gritshield::register_page!(HttpMethod::GET, handler);
