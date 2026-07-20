use std::sync::Arc;
use gritshield::core::ioc::AutoWire; 
use gritshield::{GritComponent, WireContainer};

// Mock Dependencies for Testing
#[derive(Clone)]
pub struct MockDatabase {
    pub url: String,
}

#[derive(Clone)]
pub struct MockLogger;

// Mock Component that requires both
#[derive(Clone, GritComponent)]
pub struct MockService {
    pub db: Arc<MockDatabase>,
    pub logger: Arc<MockLogger>,
}

// // ==============================================================================
// // TEST PARADIGM A: DYNAMIC ENGINE / AUTO-WIRE GRAPH VERIFICATION
// // ==============================================================================
#[test]
fn test_dynamic_di_auto_wire_verification() {
    // Submit mock components using framework's provider macro
    gritshield::inject!(MockDatabase, MockDatabase { url: "sqlite::memory:".into() });
    gritshield::inject!(MockLogger, MockLogger);

    // Verify that the graph passes verification successfully
    let verification_result = AutoWire::verify();
    assert!(
        verification_result.is_ok(),
        "Dynamic DI validation failed: {:?}",
        verification_result.err()
    );

    // Boot the container context
    AutoWire::boot_di_container();

    // Pull the resolved service instance out of the global CONTEXT
    let resolved_service = gritshield::core::ioc::CONTEXT
        .resolve::<MockService>();
        
    assert!(resolved_service.is_some(), "Failed to dynamically resolve MockService out of active state context");
    assert_eq!(resolved_service.unwrap().db.url, "sqlite::memory:");
}

// ==============================================================================
// TEST PARADIGM B: STRICT COMPILE-TIME CONTAINER STRUCTURE
// ==============================================================================
#[derive(Clone, WireContainer)]
pub struct TestContainer {
    pub db: Arc<MockDatabase>,
    pub logger: Arc<MockLogger>,
}

#[test]
fn test_strict_compile_time_wiring() {
    // Assemble the container context manually
    let container = TestContainer {
        db: Arc::new(MockDatabase { url: "postgres://localhost".into() }),
        logger: Arc::new(MockLogger),
    };

    // Statically wire up the component using the generated constructor
    let service = MockService::compile_time_wire(&container);

    // Assert that fields match structural constraints
    assert_eq!(service.db.url, "postgres://localhost");
}

#[cfg(test)]
mod tests {
    use gritshield::{core::CONTEXT, mock};

use super::*;

    #[derive(Clone)]
    pub struct MockRedisService {
        pub connection_string: String,
    }

    impl MockRedisService {
        pub fn new() -> Self {
            Self {
                connection_string: "mock://localhost:6379".to_string(),
            }
        }
    }

    // Mark it injectable so runtime bounds pass
    gritshield::mark_injectable!(MockRedisService);

    #[test]
    fn test_mock_injection() {
        // Inject mock into the global CONTEXT cache before resolving
        mock!(MockRedisService, MockRedisService::new());

        // Resolve RedisService — hits fast-path in CONTEXT.dependencies
        let redis = CONTEXT.resolve::<MockRedisService>().unwrap();
        
        assert_eq!(redis.connection_string, "mock://localhost:6379");
    }

    #[test]
    fn test_export_diagram() {
        let mermaid_md = AutoWire::export_mermaid();
        println!("Mermaid Output:\n{}", mermaid_md);
    }
}