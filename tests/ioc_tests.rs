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