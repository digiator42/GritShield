use std::time::Duration;

use gritshield::deps::async_trait;
use gritshield::http::response::HttpStatus;
use gritshield::http::{Cookie, SameSite};
use gritshield::middleware::{AfterRequestHook, Middleware, MiddlewareResult};
use gritshield::prelude::*;
use gritshield::render;
use serde_json::{json, Map, Value};

use crate::model::{validate_definition, FormDef};
use crate::store;
use crate::validate::validate_step;

// ─────────────────────────────────────────────────────────────────────────────
// Request lifecycle showcase
// ─────────────────────────────────────────────────────────────────────────────

/// Middleware run for every request.
/// - `on_request`: the `/api` mutating endpoints only accept JSON, otherwise
///   the request short-circuits here (415) before any handler runs.
/// - `on_response`: runs for every response (including rejections and 404s)
///   and stamps a header, demonstrating the unified response funnel.
pub struct ApiGuard;

#[async_trait]
impl Middleware for ApiGuard {
    async fn on_request(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        if ctx.req.path.starts_with("/api/")
            && matches!(
                ctx.req.method,
                HttpMethod::POST | HttpMethod::PUT | HttpMethod::PATCH
            )
        {
            let ct = ctx.content_type.clone().unwrap_or_default();
            if !ct.starts_with("application/json") {
                return MiddlewareResult::Error(Response::new(
                    415,
                    Sanitizer::trust("<h1>415 Unsupported Media Type</h1><p>Content-Type must be application/json</p>"),
                ));
            }
        }
        MiddlewareResult::Next(None)
    }

    async fn on_response(&self, _ctx: &RequestContext, res: &mut Response) {
        res.headers.push(("x-powered-by".to_string(), "formforge".to_string()));
    }
}

/// After-hook: observable after every completed request. Tallies global
/// request counts and bumps a form's response counter exactly when a step
/// submission finished with a 200.
pub struct StatsHook;

#[async_trait]
impl AfterRequestHook for StatsHook {
    async fn call(&self, ctx: &RequestContext, status: u16, _duration: Duration) {
        let path = ctx.req.path.clone();
        let is_success = status == 200 && path.starts_with("/api/answers/");
        if let Ok(mut store) = store::store().lock() {
            store.total_requests += 1;
            if is_success {
                let slug = path.trim_start_matches("/api/answers/").trim_matches('/');
                *store.responses.entry(slug.to_string()).or_insert(0) += 1;
                store.total_completed += 1;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pages (rendered through the shared root::layout::main_layout by render!)
// ─────────────────────────────────────────────────────────────────────────────

pub async fn home(ctx: RequestContext) -> Response {
    let forms = store::list_forms();
    render!(ctx, "FormForge", html! {
        h1 { "Multi-step forms with conditional validation" }
        p.muted { "Every form is stored in memory, validated server-side on each step, and served by gritshield 0.3." }
        h2 { "Forms" }
        @if forms.is_empty() {
            p { "No forms yet — " a href="/new" { "create your first one" } " in about a minute." }
        } @else {
            table {
                tr { th { "Title" } th { "Steps" } th { "Fields" } th { "Responses" } th { "Link" } }
                @for (slug, f, responses) in &forms {
                    tr {
                        td { (f.title) }
                        td { (f.steps.len()) }
                        td { (f.field_count()) }
                        td { (responses) }
                        td { a href=(format!("/f/{}", slug)) { "preview" } }
                    }
                }
            }
        }
        p { a href="/stats" { "Request lifecycle stats" } }
    })
}

pub async fn builder_page(ctx: RequestContext) -> Response {
    render!(ctx, "FormForge · Builder", html! {
        h1 { "Build a multi-step form" }
        p.muted {
            "Add steps; each step holds fields. Tick "
            em { "required" }
            " and optionally add a condition — the field is only required/checked when "
            em { "condition field equals the given value" }
            " (e.g. company site required only when segment equals enterprise)."
        }
        form id="meta" {
            label { "Slug" input id="slug" placeholder="lead-qualification" pattern="[a-z0-9-]+"; }
            label { "Title" input id="title" placeholder="Lead Qualification"; }
        }
        div id="steps" {}
        button.btn.ghost type="button" onclick="addStep()" { "+ Add step" }
        button.btn type="button" onclick="saveForm()" { "Save & preview form" }
        pre.err id="out" {}
        script { (maud::PreEscaped(BUILDER_JS)) }
    })
}

pub async fn form_page(ctx: RequestContext) -> Response {
    render!(ctx, "FormForge", html! {
        h1 id="ftitle" { "Loading…" }
        p.muted id="fprog" {}
        form id="fform" {}
        button.btn.ghost id="fback" { "← Back" }
        button.btn id="fnext" { "Continue →" }
        div id="fmsg" {}
        script { (maud::PreEscaped(FORM_JS)) }
    })
}

pub async fn stats_page(ctx: RequestContext) -> Response {
    let (total_requests, total_completed, top) = {
        let shared = store::store();
        let store = shared.lock().unwrap();
        let total_requests = store.total_requests;
        let total_completed = store.total_completed;
        let mut top: Vec<(String, usize)> = store.responses.iter().map(|(k, v)| (k.clone(), *v)).collect();
        top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        (total_requests, total_completed, top)
    };
    render!(ctx, "FormForge · Stats", html! {
        h1 { "Request lifecycle stats" }
        p { "Numbers driven by the global " code { "StatsHook" } " (an " code { "AfterRequestHook" } ") that observes every completed request." }
        ul {
            li { "Requests served: " strong { (total_requests) } }
            li { "Step validations that passed: " strong { (total_completed) } " total across " strong { (top.len()) } " forms" }
        }
        @if top.is_empty() {
            p { "No completions yet." }
        } @else {
            table {
                tr { th { "Form" } th { "Responses" } }
                @for (slug, n) in &top {
                    tr { td { a href=(format!("/f/{}", slug)) { (slug) } } td { (n) } }
                }
            }
            p.muted { "(passed) = a successful 200 return from /api/answers/:slug; failed validations answered 422 and are not counted." }
        }
    })
}

/// Flips the `theme` cookie between light and night, then redirects back to
/// the page the toggle was clicked from (falls back to `/`). Purely
/// server-side — night mode needs no JavaScript.
pub async fn theme_toggle(ctx: RequestContext) -> Response {
    let next = if ctx.get_cookie("theme").as_deref() == Some("night") {
        "light"
    } else {
        "night"
    };
    let host = ctx.header("host").unwrap_or("");
    let back = ctx
        .header("referer")
        .and_then(|referer| same_origin_path(referer, host))
        .unwrap_or_else(|| "/".to_string());
    let cookie = Cookie::new("theme", next).set_secure(false).set_same_site(SameSite::Lax);
    Response::redirect(HttpStatus::Found.code(), &back).with_cookie(cookie)
}

/// Returns the path of a same-origin referer so the toggle lands back on the
/// page it was clicked from. Absolute URLs are only honored when they belong
/// to this server (prevents open redirects); absolute cross-origin and
/// protocol-relative URLs are rejected.
fn same_origin_path(referer: &str, host: &str) -> Option<String> {
    if referer.starts_with('/') {
        return (!referer.starts_with("//")).then(|| referer.to_string());
    }
    for scheme in ["http://", "https://"] {
        if let Some(rest) = referer.strip_prefix(scheme) {
            let (authority, path) = match rest.split_once('/') {
                Some((a, p)) => (a, format!("/{p}")),
                None => (rest, "/".to_string()),
            };
            return (authority == host).then_some(path);
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON API
// ─────────────────────────────────────────────────────────────────────────────

pub async fn api_list_forms(ctx: RequestContext) -> Response {
    let _ = ctx;
    let list: Vec<Value> = store::list_forms()
        .into_iter()
        .map(|(slug, f, responses)| {
            json!({
                "slug": slug,
                "title": f.title,
                "steps": f.steps.len(),
                "fields": f.field_count(),
                "responses": responses
            })
        })
        .collect();
    Response::json(HttpStatus::Ok, &list)
}

pub async fn api_get_form(ctx: RequestContext) -> Response {
    let slug = ctx.param("slug").unwrap_or("").to_string();
    match store::get_form(&slug) {
        Some(def) => Response::json(HttpStatus::Ok, &def),
        None => Response::json_not_found(&json!({ "error": "no such form" })),
    }
}

pub async fn api_create_form(ctx: RequestContext) -> Response {
    let Some(body) = ctx.json_body().await else {
        return Response::json_bad_request(&json!({ "error": "Invalid JSON body" }));
    };
    let def: FormDef = match serde_json::from_value(body) {
        Ok(d) => d,
        Err(e) => {
            return Response::json_bad_request(&json!({ "error": format!("Invalid form definition: {e}") }))
        }
    };
    if let Err(msg) = validate_definition(&def) {
        return Response::json_unprocessable(&json!({ "error": msg }));
    }
    store::create_form(def.clone());
    Response::json(HttpStatus::Created, &json!({ "ok": true, "slug": def.slug }))
}

#[derive(serde::Deserialize)]
struct StepSubmit {
    step: usize,
    values: Map<String, Value>,
}

pub async fn api_submit_step(ctx: RequestContext) -> Response {
    let slug = ctx.param("slug").unwrap_or("").to_string();
    let Some(def) = store::get_form(&slug) else {
        return Response::json_not_found(&json!({ "error": "no such form" }));
    };
    let Some(body) = ctx.json_body().await else {
        return Response::json_bad_request(&json!({ "error": "Invalid JSON body" }));
    };
    let submit: StepSubmit = match serde_json::from_value(body) {
        Ok(s) => s,
        Err(e) => {
            return Response::json_bad_request(&json!({ "error": format!("Invalid submission: {e}") }))
        }
    };
    if submit.step >= def.steps.len() {
        return Response::json_bad_request(&json!({ "error": "step out of range" }));
    }
    let errors = validate_step(&def, submit.step, &submit.values);
    if errors.is_empty() {
        Response::json_ok(&json!({ "ok": true, "step": submit.step }))
    } else {
        Response::json_validation_error(errors)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Frontend scripts (fragments — the page chrome lives in root::layout)
// ─────────────────────────────────────────────────────────────────────────────

const BUILDER_JS: &str = r#"
function fieldRow(){
  const row=document.createElement('div'); row.className='field';
  row.innerHTML='<input class="fname" placeholder="field_name"> <input class="flabel" placeholder="Label"><br>'+
  '<select class="ftype"><option value="text">text</option><option value="number">number</option><option value="email">email</option><option value="select">select</option></select>'+
  '<input class="fopts" placeholder="options, comma, separated (for select)">'+
  '<label class="muted"><input type="checkbox" class="freq"> required</label>'+
  '<label class="muted">only when <input class="cond-on" placeholder="field_name"> equals <input class="cond-eq" placeholder="value"> (optional)</label>'+
  '<button class="btn ghost" onclick="this.closest(\'.field\').remove()">remove</button>';
  return row;
}
function stepBlock(){
  const d=document.createElement('div'); d.className='step';
  d.innerHTML='<input class="stitle" placeholder="Step title (optional)">'+
    '<button class="btn ghost" onclick="this.closest(\'.step\').appendChild(fieldRow())">+ field</button>'+
    '<button class="btn ghost" onclick="this.closest(\'.step\').remove()">remove step</button>';
  d.appendChild(fieldRow());
  return d;
}
function addStep(){ document.getElementById('steps').appendChild(stepBlock()); }
function saveForm(){
  const out=document.getElementById('out'); out.textContent='';
  const slug=document.getElementById('slug').value.trim();
  const title=document.getElementById('title').value.trim();
  if(!slug||!/^[a-z0-9-]+$/.test(slug)){ out.textContent='Slug must be lowercase letters/digits/hyphens.'; return; }
  const steps=[];
  document.querySelectorAll('#steps .step').forEach(s=>{
    const fields=[];
    s.querySelectorAll('.field').forEach(f=>{
      const name=f.querySelector('.fname').value.trim(), label=f.querySelector('.flabel').value.trim();
      if(!name||!label) return;
      const on=f.querySelector('.cond-on').value.trim();
      fields.push({
        name, label,
        type:f.querySelector('.ftype').value,
        options:f.querySelector('.fopts').value.split(',').map(x=>x.trim()).filter(Boolean),
        required:f.querySelector('.freq').checked,
        condition: on?{field:on,equals:f.querySelector('.cond-eq').value.trim()}:undefined
      });
    });
    steps.push({title:s.querySelector('.stitle').value.trim(),fields});
  });
  fetch('/api/forms',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({slug,title,steps})})
    .then(r=>r.json().then(d=>({ok:r.ok,d})))
    .then(({ok,d})=>{ if(ok){ location.href='/f/'+slug; } else { out.textContent='Error: '+(d.error||JSON.stringify(d)); } });
}
addStep();
"#;

const FORM_JS: &str = r#"
const slug=location.pathname.split('/').filter(Boolean)[1];
const answers={}; let step=0, def=null;
fetch('/api/forms/'+slug).then(r=>r.json().then(d=>({ok:r.ok,d}))).then(({ok,d})=>{
  if(!ok){ document.getElementById('ftitle').textContent='Form not found'; return; }
  def=d; document.getElementById('ftitle').textContent=d.title; renderStep();
}).catch(()=>{ document.getElementById('ftitle').textContent='Form not found'; });

function condMet(f){ return f.condition && String(answers[f.condition.field])===f.condition.equals; }
function setVal(name,v){ answers[name]=v; }
function renderStep(){
  if(!def) return;
  const s=def.steps[step], el=document.getElementById('fform'); el.innerHTML='';
  document.getElementById('fprog').textContent='Step '+(step+1)+' of '+def.steps.length+(s.title?': '+s.title:'');
  s.fields.forEach(f=>{
    const wrap=document.createElement('div'); wrap.className='field';
    wrap.style.display = (f.condition && !condMet(f)) ? 'none' : '';
    const label=document.createElement('label'); label.textContent=f.label+(f.required?' *':'');
    let input, cur=answers[f.name];
    if(f.type==='select'){
      const sel=document.createElement('select');
      f.options.forEach(o=>{ const opt=new Option(o,o); sel.add(opt); });
      sel.value=cur; sel.onchange=()=>{ setVal(f.name,sel.value); renderStep(); };
      input=sel;
    } else {
      input=document.createElement('input');
      input.type = f.type==='number'?'number':f.type==='email'?'email':'text';
      input.value=cur!==undefined?cur:'';
      input.oninput=()=>setVal(f.name,input.value);
    }
    wrap.append(label,input); el.append(wrap);
  });
  document.getElementById('fback').style.visibility=step===0?'hidden':'visible';
  document.getElementById('fnext').textContent=(step<def.steps.length-1)?'Continue →':'Submit';
}
function validateThen(go){
  const msg=document.getElementById('fmsg'); msg.innerHTML='';
  fetch('/api/answers/'+slug,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({step,values:answers})})
    .then(r=>r.json().then(d=>({ok:r.ok,d}))).then(({ok,d})=>{
      if(ok){ if(go) step++; renderStep(); }
      else {
        Object.entries(d.errors||{}).forEach(([k,errs])=>{
          const div=document.createElement('div'); div.className='err'; div.textContent=(k+': '+errs.join(' '));
          msg.append(div);
        });
        renderStep();
      }
    });
}
document.getElementById('fnext').onclick=()=>{ if(!def) return; if(step<def.steps.length-1){ validateThen(true); } else { finish(); } };
document.getElementById('fback').onclick=()=>{ if(step>0){ step--; renderStep(); } };
function finish(){
  fetch('/api/answers/'+slug,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({step,values:answers})})
    .then(r=>r.json().then(d=>({ok:r.ok,d}))).then(({ok,d})=>{
      if(ok){
        document.getElementById('fform').innerHTML='<h2>Submitted — thank you!</h2>';
        document.getElementById('fprog').textContent='All steps passed server-side validation (200).';
        document.getElementById('fnext').style.display='none'; document.getElementById('fback').style.display='none';
      } else {
        const msg=document.getElementById('fmsg'); msg.innerHTML='';
        Object.entries(d.errors||{}).forEach(([k,errs])=>{ const div=document.createElement('div'); div.className='err'; div.textContent=k+': '+errs.join(' '); msg.append(div); });
        renderStep();
      }
    });
}
"#;