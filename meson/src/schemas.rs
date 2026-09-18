//! Compile-time File trait DSL for TraitRegistry inventory.
//!
//! Concrete `valence_schema!` tables that opt into `File` register via their
//! owning crate's codegen. Including entity schemas here would double-submit
//! inventory (see record-history).

mod file_trait {
    include!("../schemas/file_valence_trait.rs");
}
