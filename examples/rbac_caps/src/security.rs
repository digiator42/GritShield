//! Roles and capability tokens - the two halves of GritShield's authorization
//! model.
//!
//! Roles are strings that travel with the session. Capabilities are types that
//! exist at compile time. Keeping the mapping between them in one macro is the
//! point of this file.

// Every type in this module is a zero-sized marker that exists only to be named
// by the capability matrix. Nothing constructs one, so `dead_code` is silenced
// once for the module instead of five times per struct.
#![allow(dead_code)]

use gritshield::declare_security_caps;

// Roles. Zero-sized marker types; the macro turns each into its `stringify!`
// name, and that string is what the session carries at runtime.
//
//   Admin
//   ├── Manager ── Viewer
//   ├── Operator
//   └── Auditor
//
// The tree itself is declared on the router in `main.rs`. These types only need
// to exist so the capability matrix below can name them.
pub struct Admin;
pub struct Manager;
pub struct Operator;
pub struct Auditor;
pub struct Viewer;

// Capability tokens: one per business action, named for what it allows rather
// than for who is allowed to do it.
//
// Note what is *not* here: a `BillingAdmin` or a `SupportAgent` role. Renaming
// a role or splitting one in two does not touch this file, and the endpoints
// that use these tokens do not change either.
pub struct ViewAuditLog;
pub struct ManageBilling;
pub struct RefundOrder;
pub struct DeleteAccount;

// The single source of truth: capability -> roles that satisfy it.
//
// Calling this once, anywhere in the crate, does three things per capability:
//
// 1. Implements `GritSecurityCheck`, the marker trait the `#[cap]` attribute
//    fences on. Write `#[cap(Typo)]` and the crate does not compile - a
//    capability that was never declared here is a build error, not a silent
//    runtime hole.
// 2. Implements `GritCapabilityRuntime`, which supplies the role slice the
//    generated `#[cap]` check evaluates against the session role.
// 3. Submits the pair to the router inventory, so the mapping shows up in the
//    admin panel's RBAC graph when the `admin` feature is enabled.
//
// Read it top to bottom as the answer to "who may do what".
declare_security_caps! {
    // Auditors exist to read the audit trail, so they can read it and nothing
    // else here.
    ViewAuditLog  => [Admin, Manager, Auditor],

    // Billing is management work. Operators are deliberately absent: they can
    // issue refunds (below) but cannot change the billing configuration.
    ManageBilling => [Admin, Manager],

    // The narrow exception that makes the point: an Operator can refund an
    // order without being able to reconfigure billing.
    RefundOrder   => [Operator],

    // Destructive, so it stays with a single role.
    DeleteAccount => [Admin],
}
