use gritshield::prelude::*;

pub const BASE_CSS: &str = r#"
:root{--ink:#1f2430;--muted:#6b7280;--accent:#4f46e5;--line:#e5e7eb;--bg:#fafafa;--surface:#ffffff;--code-bg:#eef0ff;--err:#dc2626}
:root[data-theme="night"]{--ink:#e2e8f0;--muted:#94a3b8;--accent:#93a3ff;--line:#334155;--bg:#0f172a;--surface:#1e293b;--code-bg:#1e293b;--err:#f87171}
*{box-sizing:border-box}
body{font:16px/1.5 system-ui,Segoe UI,Roboto,sans-serif;margin:0;background:var(--bg);color:var(--ink);transition:background-color .25s ease,color .25s ease}
main{max-width:880px;margin:2rem auto;padding:0 1rem}
.nav{display:flex;gap:1rem;align-items:center;padding:.9rem 1.2rem;background:var(--surface);border-bottom:1px solid var(--line);transition:background-color .25s ease}
.nav a{color:var(--accent);text-decoration:none;margin-right:.6rem}
.nav-right{margin-left:auto;font-size:.85rem}
h1{font-size:1.7rem}h2{margin-top:2rem}
a{color:var(--accent)}
table{width:100%;border-collapse:collapse;background:var(--surface);margin:1rem 0}
th,td{text-align:left;padding:.5rem .7rem;border-bottom:1px solid var(--line)}
code{background:var(--code-bg);border-radius:4px;padding:1px 4px}
input,select{font:inherit;padding:.4rem .5rem;border:1px solid var(--line);border-radius:6px;margin:.2rem 0;background:var(--surface);color:var(--ink)}
label{display:block;margin-top:.9rem;font-weight:600}
.step{border:1px solid var(--line);border-radius:10px;background:var(--surface);padding:1rem;margin:.9rem 0}
.field{border-top:1px dashed var(--line);padding:.5rem 0}
.err{color:var(--err);font-size:.85rem}
.btn{background:var(--accent);color:#fff;border:0;border-radius:8px;padding:.55rem 1rem;margin:.4rem .3rem 0 0;cursor:pointer;font:inherit}
.btn.ghost{background:transparent;color:var(--accent);border:1px solid var(--accent)}
.muted{color:var(--muted);font-size:.85rem}
#fmsg .err{margin-top:.5rem}
footer{border-top:1px solid var(--line);margin-top:3rem;padding:1rem;text-align:center;transition:background-color .25s ease}
"#;

/// Shared page chrome. `render!` in every handler awaits this function, so it
/// must be `async` and return `Markup`. Theme comes from the `theme` cookie,
/// so no JavaScript is needed for night mode — the toggle is a plain link.
pub async fn main_layout(title: &str, content: Markup, ctx: &RequestContext) -> Markup {
    let night = ctx.get_cookie("theme").as_deref() == Some("night");
    let theme_attr = if night { "night" } else { "light" };
    html! {
        (maud::DOCTYPE)
        html data-theme=(theme_attr) {
            head {
                title { (title) }
                style { (maud::PreEscaped(BASE_CSS)) }
            }
            body {
                div.nav {
                    a href="/" { "FormForge" }
                    a href="/new" { "Builder" }
                    a href="/stats" { "Stats" }
                    a.nav-right href="/theme/toggle" {
                        @if night { "Switch to light" } @else { "Night mode" }
                    }
                }
                main {
                    (content)
                }
                footer {
                    p.muted { "Crafted with gritshield 0.3 — every page runs through root::layout::main_layout via the render! macro." }
                }
            }
        }
    }
}