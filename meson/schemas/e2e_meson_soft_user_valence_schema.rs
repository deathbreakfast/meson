use valence::prelude::*;
use valence::privacy_policies::common::SYSTEM_ONLY;

// Codegen-only stub target for File.uploaded_by hops. Table is not `user`
// so Meson does not collide with a host's own User schema in a host link.

valence_schema! {
    E2eMesonSoftUser {
        repository: "https://github.com/unified-field-dev/meson",
        table: "e2e_meson_soft_user",
        version: "0.1.0",
        database: crate::embedded_surreal::DEFAULT_STORAGE,
        description: "Codegen stub for File.uploaded_by (not a product User model)",

        policies: {
            read: { allow: [SYSTEM_ONLY] },
            create: { allow: [SYSTEM_ONLY] },
            update: { allow: [SYSTEM_ONLY] },
            delete: { allow: [SYSTEM_ONLY] },
        },

        fields: [
            id: {
                r#type: FieldType::String,
                primary_key: true,
                required: true,
            },
        ],
    }
}
