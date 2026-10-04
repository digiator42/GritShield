# RBAC and Capabilities

Authorization in GritShield, from a login endpoint to five endpoints that five
different roles are allowed and denied differently.

```bash
cargo run --manifest-path examples/rbac_caps/Cargo.toml
```

No database, no Redis. Roles live in the session and capabilities are checked in
process.

| User | Role | Password |
|------|------|----------|
| `ada` | Admin | `admin123` |
| `mike` | Manager | `manager123` |
| `olga` | Operator | `operator123` |
| `audrey` | Auditor | `auditor123` |
| `vic` | Viewer | `viewer123` |

```bash
curl -c jar -X POST http://127.0.0.1:8084/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"olga","password":"operator123"}'
curl -b jar -i http://127.0.0.1:8084/api/logs
```

## The two halves

| | Roles | Capabilities |
|---|---|---|
| What it is | a string in the session | a type in your source |
| Declared with | the login handler, plus a tree on the router | `declare_security_caps!` |
| Checked with | `role = "Auditor"` or `ctx.has_role(..)` | `#[cap(ViewAuditLog)]` |
| Inherits | yes, through the tree | no, it names roles explicitly |
| Wrong value fails | at runtime, with a `403` | at compile time |

Roles answer "which tier is this user in". Capabilities answer "may this user
perform this action", and they are the part that survives a refactor: splitting
`Manager` into `BillingManager` and `SupportManager` is a change to one macro
invocation, and no endpoint moves.

## 1. Authentication

`AuthMiddleware` decides who the caller is, and it looks at exactly two fields
in the session: `user_id` for authentication, `role` for authorization.

```rust
ctx.login_user_id(username);          // makes AuthMiddleware trust this session
ctx.set_session_data("role", role);   // makes ctx.has_role() answer for it
```

Both calls matter. Log in and skip the second and the caller is authenticated
but authorized for nothing - every `role` and `#[cap]` check fails with a `403`.

```rust
let auth = AuthMiddleware::new_session(
    PUBLIC_PATHS.iter().map(|p| p.to_string()).collect(),
    None,   // no redirect target -> 401 instead of 303
);

let router = Router::new()
    .add_role_inheritance("Admin", vec!["Manager", "Operator", "Auditor"])
    .add_role_inheritance("Manager", vec!["Viewer"])
    .add_middleware(auth);
```

The public list is the only unauthenticated surface. Everything else answers
`401` without a session:

```
$ curl -i http://127.0.0.1:8084/api/logs
HTTP/1.1 401 Unauthorized
```

Pass `Some("/auth/login")` as the second argument and that becomes a `303` to
the login page instead, which is what you want in a browser and what you do not
want in an API client.

`enable_csrf` defaults to `false`. Turn it on and every `POST`, `PUT`, `PATCH`
and `DELETE` must carry the session's `csrf_token` in an `x-csrf-token` header
or a `csrf_token` form field, or it answers `403`. Every `curl` below would
start failing, which is exactly the intent. The security example covers the
token flow.

### Logging out

There is no logout endpoint to write. `AuthMiddleware` intercepts the literal
path `/logout` for **any** method, destroys the session and redirects to `/`
when no redirect target is configured:

```
$ curl -b jar -i http://127.0.0.1:8084/logout
HTTP/1.1 303 See Other
location: /
```

Because it is method-agnostic, a `<img src="/logout">` on another origin ends a
session. That is the CSRF guard's job, which is another reason not to leave
`enable_csrf` off.

## 2. Roles and inheritance

`add_role_inheritance(parent, children)` builds a tree that `ctx.has_role`
walks recursively. The example declares:

```
        [Admin]
       /   |    \
 [Manager] [Operator] [Auditor]
     |
 [Viewer]
```

- `has_role` matches the role itself first, then walks the tree.
- `Admin` reaches everything, including `Viewer` through `Manager`.
- `Operator` reaches nothing. Siblings do not inherit from each other, and
  `Operator` has no children.
- `has_role` also grants everything to the role literally named `SuperAdmin`,
  whatever the tree says.

This is why `mike` (Manager) is refused `GET /api/reports` below: `Auditor` is
a *sibling* of `Manager`, not an ancestor. `ada` (Admin) is allowed, because
`Admin` inherits `Auditor`.

`ctx.has_fixed_role` is a separate, non-tree check with a hardcoded
`Admin > Operator > Auditor` ladder baked into the framework. This example does
not use it, because a hierarchy you cannot see from `main.rs` is a hierarchy you
cannot refactor.

## 3. Three ways to guard a route

```rust
// a) the attribute - enforced before the handler is called
#[get("/reports", role = "Auditor")]
pub async fn reports(ctx: RequestContext) -> Response { /* ... */ }

// b) the inline guard - for requirements the attribute cannot express
#[get("/team/roster")]
pub async fn roster(ctx: RequestContext) -> ShieldResult<Response> {
    ctx.require_role("Manager")?;
    Ok(Response::ok("..."))
}

// c) a capability token
#[get("/logs")]
#[cap(ViewAuditLog)]
pub async fn audit_logs(ctx: RequestContext) -> Response { /* ... */ }
```

(a) runs in the connection loop before your code, so a handler cannot forget it
and never learns it was called. (b) is the same check by hand, returning
`ShieldResult<()>`, for when the answer depends on the request - a resource
owner, a tenant, a field in the body. (c) is the one to reach for by default.

The three produce different denial bodies, which is worth knowing when you are
debugging a `403`:

| Guard | Body |
|-------|------|
| `role = "Auditor"` | `{"error":"Access Denied: Missing required operational role clearance 'Auditor'."}` |
| `ctx.require_role(..)?` | `403 Forbidden` |
| `#[cap(..)]` | `403 Forbidden - Insufficient capability privileges` |

## 4. Capabilities

Declare the tokens once, in one place:

```rust
declare_security_caps! {
    ViewAuditLog  => [Admin, Manager, Auditor],
    ManageBilling => [Admin, Manager],
    RefundOrder   => [Operator],
    DeleteAccount => [Admin],
}
```

Then use the type, never the role string:

```rust
use crate::security::{DeleteAccount, ManageBilling, RefundOrder, ViewAuditLog};

#[get("/logs")]
#[cap(ViewAuditLog)]
pub async fn audit_logs(ctx: RequestContext) -> Response { /* ... */ }

#[post("/refunds")]
#[cap(ManageBilling, RefundOrder)]
pub async fn refund(ctx: RequestContext) -> Response { /* ... */ }

#[delete("/accounts/:id")]
#[cap(DeleteAccount)]
pub async fn delete_account(ctx: RequestContext) -> Response { /* ... */ }
```

Multiple capabilities in one attribute are **OR**: access is granted if the
session role satisfies any one of them. `POST /api/refunds` therefore admits
`Admin` and `Manager` through `ManageBilling` and `Operator` through
`RefundOrder`, while `Auditor` and `Viewer` are refused. That is the case
capabilities exist for: an operator can refund an order without being able to
reconfigure billing, and the endpoint never names either role.

### The compile-time part

A capability that was never declared does not compile. Define the type but leave
it out of the macro, and both fences fire:

```text
error[E0277]: the trait bound `NotDeclared: GritSecurityCheck` is not satisfied
error[E0277]: the trait bound `NotDeclared: GritCapabilityRuntime` is not satisfied
```

Misspell the token and you get `cannot find type ViewAuditlog in this scope`
instead. Either way there is no way to ship an endpoint whose capability was
never bound to any role.

`declare_security_caps!` also submits each pair to the router inventory, so the
matrix shows up in the admin panel's RBAC graph when the `admin` feature is on.

## The whole matrix, measured

Every cell below is real output from this example - log in as each user and call
each endpoint:

| Role | `GET /api/reports`<br>`role = "Auditor"` | `GET /api/team/roster`<br>`Manager` | `GET /api/logs`<br>`ViewAuditLog` | `POST /api/refunds`<br>`ManageBilling \| RefundOrder` | `DELETE /api/accounts/:id`<br>`DeleteAccount` |
|------|------|------|------|------|------|
| **Admin** | 200 | 200 | 200 | 201 | 200 |
| **Manager** | 403 | 200 | 200 | 201 | 403 |
| **Operator** | 403 | 403 | 403 | 201 | 403 |
| **Auditor** | 200 | 403 | 200 | 403 | 403 |
| **Viewer** | 403 | 403 | 403 | 403 | 403 |

Two results in that table are only explicable if you read the tree and the macro
together:

- `Manager` is refused `reports`, because `Auditor` is `Admin`'s child, not
  `Manager`'s.
- `Auditor` is refused `roster`, for the mirror-image reason.

## Sharp edges

- **One `#[cap(..)]` attribute per handler.** The macro strips the first one and
  leaves the rest, so a second attribute is a hard error:
  `cannot find attribute 'cap' in this scope`. Put every capability in one
  attribute: `#[cap(A, B)]`.
- **`#[cap]` and `role = ".."` on the same route both apply**, in that order,
  and satisfying one is not enough. On `#[get("/logs", role = "Viewer")]` with
  `#[cap(ViewAuditLog)]`: `vic` clears the role but is refused the capability,
  `audrey` clears the capability but is refused the role, `mike` passes both
  because `Manager` inherits `Viewer`, and `ada` passes both.
- **Role strings are compared exactly.** `Admin` and `admin` are different
  roles, and the session is the only place the difference can be introduced. A
  typo in the login handler produces a `403` on every guarded route, not a
  startup error.
- **A role in the tree but in no capability list grants nothing through
  `#[cap]`.** Inheritance is only consulted for roles a capability names.
- **Capabilities are not a replacement for resource checks.** `#[cap]` answers
  "may an Admin delete an account"; nothing in this framework answers "is this
  Admin deleting their own account". That check is still yours.
- **`declare_security_caps!` expands to `impl` blocks**, so call it once per
  capability, anywhere in the crate. `src/security.rs` here, not `main.rs` -
  contrary to what the older docs suggest.

## Files

| File | What it shows |
|------|---------------|
| `src/security.rs` | Roles, capability tokens, and the one matrix that binds them |
| `src/auth.rs` | Login writing `user_id` and `role` into the session |
| `src/api.rs` | The four authorization styles side by side |
| `src/main.rs` | Public paths, inheritance tree, middleware wiring |
