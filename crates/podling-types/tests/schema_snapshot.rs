//! The JSON Schemas are the contract with non-Rust consumers. Any change to an
//! artifact type changes a snapshot here and fails this test.
//!
//! When the change is intended: review it with `cargo insta review`, accept
//! it, and bump `SCHEMA_VERSION` in `src/envelope.rs`.

use podling_types::schema;

#[test]
fn schemas_match_snapshots() {
    for (kind, schema) in schema::all() {
        insta::assert_json_snapshot!(kind.as_str(), schema);
    }
}

#[test]
fn schema_version_is_pinned() {
    // Update together with the snapshots above.
    assert_eq!(podling_types::SCHEMA_VERSION, 1);
}
