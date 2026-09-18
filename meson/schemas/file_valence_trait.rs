// Shared Valence File trait — metadata for uploadable objects.
// Consumers declare traits: [File] and vendor this file under their schemas/.
// Soft uploaded_by → user (codegen stub model) so Meson depends on Valence only.

use valence::prelude::*;

valence_trait_schema! {
    File {
        repository: "https://github.com/unified-field-dev/meson",
        fields: [
            file_name: { r#type: FieldType::String, required: true },
            file_extension: { r#type: FieldType::String, required: true },
            mime_type: { r#type: FieldType::String, required: true },
            size_bytes: { r#type: FieldType::Integer, required: true },
            storage_path: { r#type: FieldType::String, required: true },
            file_status: {
                // Upload defaults to `pending_virus_scan` when virus scan is on;
                // clean promote → `available`; dirty → `quarantined`.
                // `virus_scan_complete` remains reserved / unused this Ship.
                r#type: FieldType::Enum(&[
                    "available", "pending_virus_scan", "virus_scan_complete", "quarantined",
                ]),
                required: true,
            },
            uploaded_by: { r#type: FieldType::Record("user"), required: true },
            uploaded_at: { r#type: FieldType::DateTime, required: true },
        ],
        connections: [
            uploaded_by: {
                table: "user",
                cardinality: HasOne,
                required: true,
                on_delete: Cascade,
                // Codegen stub for the `user` table hop. RecordIds stay `user:<id>`;
                // each consuming host overrides `model:` to resolve its own real
                // User type at runtime.
                model: "crate::generated::E2eMesonSoftUser",
            },
        ],
    }
}
