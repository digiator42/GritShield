# GritShield CLI

Command-line developer utility tool for building robust, secure web services with the GritShield framework kernel.

## Installation

Install the binary executable globally using Cargo:
```bash
cargo install gritshield_cli
```

Then 

```bash
# Create a fresh project interactively
gritshield new secure_app
cd secure_app

# Add a new admin controller instantly
gritshield gen controller admin_metrics

# Scaffold an account data model structure
gritshield generate model operator_profile

# Generate a timestamped SQL migration blueprint
gritshield gen migration add_audit_logs_table
```