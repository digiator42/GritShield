/// The central layout type used to bind roles to capabilities at compile time.
pub struct SecurityRegistry;

/// An immutable compile-time proof that an organizational Role satisfies a specific Capability constraint.
pub trait RoleGrantsCapability<Role, Cap> {}

pub trait GritSecurityCheck {}

pub trait GritCapabilityRuntime {
    fn name() -> &'static str;
    fn allowed_roles() -> &'static [&'static str];
}
