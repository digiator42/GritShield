/// The central layout type used to bind roles to capabilities at compile time.
pub struct SecurityRegistry;

/// An immutable compile-time proof that an organizational Role satisfies a specific Capability constraint.
pub trait RoleGrantsCapability<Role, Cap> {}

// ==========================================
// Pre-configured Standard Roles
// ==========================================
pub struct Admin;
pub struct Manager;
pub struct Auditor;
pub struct User;
pub struct Guest;

// ==========================================
// Pre-configured Standard Capabilities
// ==========================================
pub struct Read;
pub struct Write;
pub struct Delete;
pub struct ManageBilling;
pub struct DeleteUser;
pub struct ViewLogs;
pub struct SystemAdmin;