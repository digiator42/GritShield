use gritshield::declare_security_caps;

pub struct Admin;
pub struct Manager;
pub struct Developer;
pub struct Tester;
pub struct Viewer;

// ============================================================
// Security Capability Tokens
// ============================================================
pub struct IssueEdit;
pub struct IssueCreate;
pub struct IssueDelete;
pub struct ProjectAdmin;
pub struct ViewBoard;

// One single source of truth grouped by capability matching your endpoint attributes!
declare_security_caps! {
    IssueDelete => [Admin],
    IssueCreate    => [Admin],
    ViewBoard      => [Manager, Auditor],
}
